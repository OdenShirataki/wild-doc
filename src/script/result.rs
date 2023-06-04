use deno_runtime::{
    deno_napi::v8::{self, HandleScope, PropertyAttribute},
    worker::MainWorker,
};
use semilattice_database_session::{Condition, Order, OrderKey, Session, SessionDatabase};
use std::{collections::HashMap, ffi::c_void};

use super::Script;
use super::{get_wddb, stack};

fn get_collection_id<'s>(
    scope: &mut v8::HandleScope<'s>,
    this: v8::Local<v8::Object>,
) -> Option<i32> {
    if let Some(val) = v8::String::new(scope, "collection_id")
        .and_then(|s| this.get(scope, s.into()))
        .and_then(|s| s.to_int32(scope))
    {
        Some(val.value())
    } else {
        None
    }
}

fn get_row<'s>(scope: &mut v8::HandleScope<'s>, this: v8::Local<v8::Object>) -> Option<i64> {
    if let Some(val) = v8::String::new(scope, "row")
        .and_then(|s| this.get(scope, s.into()))
        .and_then(|s| s.to_big_int(scope))
    {
        Some(val.i64_value().0)
    } else {
        None
    }
}

fn get_session<'s>(
    scope: &mut v8::HandleScope<'s>,
    this: v8::Local<v8::Object>,
) -> Option<&'s mut Session> {
    if let Some(val) = v8::String::new(scope, "session").and_then(|s| this.get(scope, s.into())) {
        let session = unsafe { v8::Local::<v8::External>::cast(val) }.value() as *mut Session;
        Some(unsafe { &mut *session })
    } else {
        None
    }
}

fn depends(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let this = args.this();
    if let (Some(db), Some(collection_id), Some(row)) = (
        get_wddb(scope),
        get_collection_id(scope, this),
        get_row(scope, this),
    ) {
        if let Ok(db) = db.read() {
            let key = if args.length() > 0 {
                args.get(0).to_string(scope)
            } else {
                None
            };
            let array = depends_array(scope, &db, key, collection_id, row, None);
            rv.set(array.into());
        }
    }
}

fn session_depends(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let this = args.this();
    if let (Some(db), Some(collection_id), Some(row), Some(session)) = (
        get_wddb(scope),
        get_collection_id(scope, this),
        get_row(scope, this),
        get_session(scope, this),
    ) {
        if let Ok(db) = db.read() {
            let key = if args.length() > 0 {
                args.get(0).to_string(scope)
            } else {
                None
            };
            let array = depends_array(scope, &db, key, collection_id, row, Some(&session));
            rv.set(array.into());
        }
    }
}

fn depends_array<'a>(
    scope: &mut v8::HandleScope<'a>,
    db: &SessionDatabase,
    key: Option<v8::Local<'a, v8::String>>,
    collection_id: i32, //u32?
    row: i64,
    session: Option<&Session>,
) -> v8::Local<'a, v8::Array> {
    let (collection_id, row) = if row < 0 {
        (-collection_id, -row)
    } else {
        (collection_id, row)
    };
    let depends = if let Some(key_name) = key {
        let key_name = key_name.to_rust_string_lossy(scope);
        db.depends_with_session(Some(&key_name), collection_id, row as u32, session)
    } else {
        db.depends_with_session(None, collection_id, row as u32, session)
    };

    let array = v8::Array::new(scope, depends.len() as i32);
    if let (Some(v8str_key), Some(v8str_collection), Some(v8str_row)) = (
        v8::String::new(scope, "key"),
        v8::String::new(scope, "collection"),
        v8::String::new(scope, "row"),
    ) {
        let mut index: u32 = 0;
        for d in depends {
            if let (Some(key), Some(collection)) = (
                v8::String::new(scope, d.key()),
                db.collection(d.collection_id()),
            ) {
                if let Some(collection_name) = v8::String::new(scope, collection.name()) {
                    let depend = v8::Object::new(scope);
                    depend.define_own_property(
                        scope,
                        v8str_key.into(),
                        key.into(),
                        PropertyAttribute::READ_ONLY,
                    );
                    depend.define_own_property(
                        scope,
                        v8str_collection.into(),
                        collection_name.into(),
                        PropertyAttribute::READ_ONLY,
                    );
                    let row = v8::BigInt::new_from_i64(scope, d.row() as i64); //TODO u32 integer
                    depend.define_own_property(
                        scope,
                        v8str_row.into(),
                        row.into(),
                        PropertyAttribute::READ_ONLY,
                    );

                    array.set_index(scope, index, depend.into());
                    index += 1;
                }
            }
        }
    }

    array
}

fn field(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let this = args.this();

    if let (Some(db), Some(collection_id), Some(row)) = (
        get_wddb(scope),
        get_collection_id(scope, this),
        get_row(scope, this),
    ) {
        if let Some(field_name) = args.get(0).to_string(scope) {
            let field_name = field_name.to_rust_string_lossy(scope);
            if let Some(data) = db.read().unwrap().collection(collection_id) {
                if let Some(str) = v8::String::new(
                    scope,
                    std::str::from_utf8(data.field_bytes(row as u32, &field_name)).unwrap(),
                ) {
                    rv.set(str.into());
                }
            }
        }
    }
}

fn session_field(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let this = args.this();

    if let (Some(db), Some(collection_id), Some(row), Some(session)) = (
        get_wddb(scope),
        get_collection_id(scope, this),
        get_row(scope, this),
        get_session(scope, this),
    ) {
        if let Some(field_name) = args.get(0).to_string(scope) {
            let field_name = field_name.to_rust_string_lossy(scope);
            if let Some(data) = db.read().unwrap().collection(collection_id) {
                let bytes = session.collection_field_bytes(&data, row, &field_name);
                if let Some(str) = v8::String::new(scope, std::str::from_utf8(&bytes).unwrap()) {
                    rv.set(str.into());
                }
            }
        }
    }
}

fn make_order(sort: &str) -> Vec<Order> {
    let mut orders = vec![];
    if sort.len() > 0 {
        for o in sort.trim().split(",") {
            let o = o.trim();
            let is_desc = o.ends_with(" DESC");
            let o_split: Vec<&str> = o.split(" ").collect();
            let field = o_split[0];
            let order_key = if field.starts_with("field.") {
                if let Some(field_name) = field.strip_prefix("field.") {
                    Some(OrderKey::Field(field_name.to_owned()))
                } else {
                    None
                }
            } else {
                match field {
                    "serial" => Some(OrderKey::Serial),
                    "row" => Some(OrderKey::Row),
                    "term_begin" => Some(OrderKey::TermBegin),
                    "term_end" => Some(OrderKey::TermEnd),
                    "last_update" => Some(OrderKey::LastUpdated),
                    _ => None,
                }
            };
            if let Some(order_key) = order_key {
                orders.push(if is_desc {
                    Order::Desc(order_key)
                } else {
                    Order::Asc(order_key)
                });
            }
        }
    }
    orders
}

fn set_values<'s>(
    scope: &mut HandleScope<'s>,
    collection_id: i32,
    collection_name: &str,
    row: i64,
    activity: i32,
    term_begin: u64,
    term_end: u64,
) -> v8::Local<'s, v8::Object> {
    let obj = v8::Object::new(scope);

    if let Some(key) = v8::String::new(scope, "row") {
        let row = v8::BigInt::new_from_i64(scope, row);
        obj.define_own_property(scope, key.into(), row.into(), PropertyAttribute::READ_ONLY);
    }
    if let (Some(collection_name), Some(key)) = (
        v8::String::new(scope, collection_name),
        v8::String::new(scope, "collection"),
    ) {
        obj.define_own_property(
            scope,
            key.into(),
            collection_name.into(),
            PropertyAttribute::READ_ONLY,
        );
    }
    if let Some(key) = v8::String::new(scope, "collection_id") {
        let collection_id = v8::Integer::new(scope, collection_id as i32);
        obj.define_own_property(
            scope,
            key.into(),
            collection_id.into(),
            PropertyAttribute::READ_ONLY,
        );
    }
    if let Some(key) = v8::String::new(scope, "activity") {
        let activity = v8::Integer::new(scope, activity);
        obj.define_own_property(
            scope,
            key.into(),
            activity.into(),
            PropertyAttribute::READ_ONLY,
        );
    }
    if let (Some(term_begin), Some(key)) = (
        v8::Date::new(scope, (term_begin as f64) * 1000.0),
        v8::String::new(scope, "term_begin"),
    ) {
        obj.define_own_property(
            scope,
            key.into(),
            term_begin.into(),
            PropertyAttribute::READ_ONLY,
        );
    }
    if term_end > 0 {
        if let (Some(term_end), Some(key)) = (
            v8::Date::new(scope, (term_end as f64) * 1000.0),
            v8::String::new(scope, "term_end"),
        ) {
            obj.define_own_property(
                scope,
                key.into(),
                term_end.into(),
                PropertyAttribute::READ_ONLY,
            );
        }
    }

    obj
}

fn set_serial<'s>(scope: &mut HandleScope<'s>, object: v8::Local<'s, v8::Object>, serial: u32) {
    if let Some(v8str_serial) = v8::String::new(scope, "serial") {
        let serial = v8::Integer::new_from_unsigned(scope, serial);
        object.define_own_property(
            scope,
            v8str_serial.into(),
            serial.into(),
            PropertyAttribute::READ_ONLY,
        );
    }
}
fn set_last_update<'s>(
    scope: &mut HandleScope<'s>,
    object: v8::Local<'s, v8::Object>,
    last_update: u64,
) {
    if let Some(v8str_last_update) = v8::String::new(scope, "last_update") {
        if let Some(last_update) = v8::Date::new(scope, (last_update as f64) * 1000.0) {
            object.define_own_property(
                scope,
                v8str_last_update.into(),
                last_update.into(),
                PropertyAttribute::READ_ONLY,
            );
        }
    }
}
fn set_uuid<'s>(scope: &mut HandleScope<'s>, object: v8::Local<'s, v8::Object>, uuid: &str) {
    if let Some(v8str_uuid) = v8::String::new(scope, "uuid") {
        let uuid = v8::String::new(scope, uuid).unwrap();
        object.define_own_property(
            scope,
            v8str_uuid.into(),
            uuid.into(),
            PropertyAttribute::READ_ONLY,
        );
    }
}

pub(super) fn result(
    script: &mut Script,
    worker: &mut MainWorker,
    attributes: &HashMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>,
    search_map: &HashMap<String, (i32, Vec<Condition>)>,
) {
    let search = crate::attr_parse_or_static_string(worker, attributes, b"search");
    let var = crate::attr_parse_or_static_string(worker, attributes, b"var");

    let orders = make_order(&crate::attr_parse_or_static_string(
        worker, attributes, b"sort",
    ));

    let scope = &mut worker.js_runtime.handle_scope();
    let context = scope.get_current_context();
    let scope = &mut v8::ContextScope::new(scope, context);

    let obj = v8::Object::new(scope);

    if search != "" && var != "" {
        if let Some(v8str_var) = v8::String::new(scope, &var) {
            let return_obj = v8::Array::new(scope, 0);
            if let (Some(v8str_field), Some(v8str_depends), Some((collection_id, conditions))) = (
                v8::String::new(scope, "field"),
                v8::String::new(scope, "depends"),
                search_map.get(&search),
            ) {
                let collection_id = *collection_id;
                let mut session_maybe_has_collection = None;
                for i in (0..script.sessions.len()).rev() {
                    if let Some(_) = script.sessions[i].0.temporary_collection(collection_id) {
                        session_maybe_has_collection = Some(i);
                        break;
                    }
                }

                if let Some(collection) = script
                    .database
                    .clone()
                    .read()
                    .unwrap()
                    .collection(collection_id)
                {
                    if let Some(session_index) = session_maybe_has_collection {
                        let session = &mut script.sessions[session_index].0;
                        let addr = session as *mut Session as *mut c_void;
                        let v8ext_session = v8::External::new(scope, addr);
                        if let (
                            Some(v8func_field),
                            Some(v8func_depends),
                            Some(v8str_session),
                            Some(temporary_collection),
                        ) = (
                            v8::Function::new(scope, session_field),
                            v8::Function::new(scope, session_depends),
                            v8::String::new(scope, "session"),
                            session.temporary_collection(collection_id),
                        ) {
                            let mut i = 0;
                            let search = session.search(collection_id, conditions);
                            for row in script
                                .database
                                .clone()
                                .read()
                                .unwrap()
                                .result_session(search, orders)
                                .unwrap()
                            {
                                let (activity, term_begin, term_end) =
                                    if let Some(tr) = temporary_collection.get(&row) {
                                        (tr.activity() as i32, tr.term_begin(), tr.term_end())
                                    } else {
                                        let row = row as u32;
                                        (
                                            collection.activity(row) as i32,
                                            collection.term_begin(row),
                                            collection.term_begin(row),
                                        )
                                    };

                                let obj = set_values(
                                    scope,
                                    collection_id,
                                    collection.name(),
                                    row,
                                    activity,
                                    term_begin,
                                    term_end,
                                );

                                set_serial(
                                    scope,
                                    obj,
                                    if row > 0 {
                                        collection.serial(row as u32)
                                    } else {
                                        0
                                    },
                                );

                                obj.define_own_property(
                                    scope,
                                    v8str_session.into(),
                                    v8ext_session.into(),
                                    PropertyAttribute::READ_ONLY,
                                );

                                obj.define_own_property(
                                    scope,
                                    v8str_field.into(),
                                    v8func_field.into(),
                                    PropertyAttribute::READ_ONLY,
                                );
                                obj.define_own_property(
                                    scope,
                                    v8str_depends.into(),
                                    v8func_depends.into(),
                                    PropertyAttribute::READ_ONLY,
                                );

                                if row > 0 {
                                    set_last_update(
                                        scope,
                                        obj,
                                        collection.last_updated(row as u32),
                                    );
                                    set_uuid(scope, obj, &collection.uuid_string(row as u32));
                                } else {
                                    if let Some(tr) = temporary_collection.get(&row) {
                                        set_uuid(scope, obj, &tr.uuid_string());
                                    }
                                }
                                return_obj.set_index(scope, i, obj.into());
                                i += 1;
                            }
                        }
                    } else {
                        if let (Some(v8func_field), Some(v8func_depends)) = (
                            v8::Function::new(scope, field),
                            v8::Function::new(scope, depends),
                        ) {
                            let mut search = script.database.read().unwrap().search(collection);
                            for c in conditions {
                                search = search.search(c.clone());
                            }

                            let rows = script
                                .database
                                .clone()
                                .read()
                                .unwrap()
                                .result(search, &orders)
                                .unwrap();
                            let mut i = 0;
                            for row in rows {
                                let obj = set_values(
                                    scope,
                                    collection_id as i32,
                                    collection.name(),
                                    row as i64,
                                    collection.activity(row) as i32,
                                    collection.term_begin(row),
                                    collection.term_end(row),
                                );

                                set_serial(scope, obj, collection.serial(row));

                                set_last_update(scope, obj, collection.last_updated(row));
                                set_uuid(scope, obj, &collection.uuid_string(row));

                                obj.define_own_property(
                                    scope,
                                    v8str_field.into(),
                                    v8func_field.into(),
                                    PropertyAttribute::READ_ONLY,
                                );
                                obj.define_own_property(
                                    scope,
                                    v8str_depends.into(),
                                    v8func_depends.into(),
                                    PropertyAttribute::READ_ONLY,
                                );

                                return_obj.set_index(scope, i, obj.into());
                                i += 1;
                            }
                        }
                    }
                }
            }
            obj.set(scope, v8str_var.into(), return_obj.into());
        }
    }
    stack::push(context, scope, obj);
}
