use deno_runtime::{
    deno_napi::v8::{self, HandleScope, READ_ONLY},
    worker::MainWorker,
};
use quick_xml::events::BytesStart;
use semilattice_database::{Condition, Database, Order, OrderKey, Session};
use std::{collections::HashMap, ffi::c_void};

use crate::xml_util;

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
        if let Ok(db) = db.clone().read() {
            let key = args.get(0).to_string(scope);
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
        if let Ok(db) = db.clone().read() {
            let key = args.get(0).to_string(scope);
            let array = depends_array(scope, &db, key, collection_id, row, Some(&session));
            rv.set(array.into());
        }
    }
}

fn depends_array<'a>(
    scope: &mut v8::HandleScope<'a>,
    db: &Database,
    key: Option<v8::Local<'a, v8::String>>,
    collection_id: i32,
    row: i64,
    session: Option<&Session>,
) -> v8::Local<'a, v8::Array> {
    let depends = if let Some(key_name) = key {
        let key_name = key_name.to_rust_string_lossy(scope);
        db.depends(Some(&key_name), collection_id, row, session)
    } else {
        db.depends(None, collection_id, row, session)
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
                    depend.define_own_property(scope, v8str_key.into(), key.into(), READ_ONLY);
                    depend.define_own_property(
                        scope,
                        v8str_collection.into(),
                        collection_name.into(),
                        READ_ONLY,
                    );
                    let row = v8::BigInt::new_from_i64(scope, d.row());
                    depend.define_own_property(scope, v8str_row.into(), row.into(), READ_ONLY);

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
            if let Some(data) = db.clone().read().unwrap().collection(collection_id) {
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
            if let Some(data) = db.clone().read().unwrap().collection(collection_id) {
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
    row: i64,
    activity: i32,
    term_begin: u64,
    term_end: u64,
) -> v8::Local<'s, v8::Object> {
    let obj = v8::Object::new(scope);

    if let (
        Some(v8str_collection_id),
        Some(v8str_row),
        Some(v8str_activity),
        Some(v8str_term_begin),
        Some(v8str_term_end),
    ) = (
        v8::String::new(scope, "collection_id"),
        v8::String::new(scope, "row"),
        v8::String::new(scope, "activity"),
        v8::String::new(scope, "term_begin"),
        v8::String::new(scope, "term_end"),
    ) {
        let row = v8::BigInt::new_from_i64(scope, row);
        obj.define_own_property(scope, v8str_row.into(), row.into(), READ_ONLY);

        let collection_id = v8::Integer::new(scope, collection_id as i32);
        obj.define_own_property(
            scope,
            v8str_collection_id.into(),
            collection_id.into(),
            READ_ONLY,
        );

        let activity = v8::Integer::new(scope, activity);
        obj.define_own_property(scope, v8str_activity.into(), activity.into(), READ_ONLY);

        if let Some(term_begin) = v8::Date::new(scope, (term_begin as f64) * 1000.0) {
            obj.define_own_property(scope, v8str_term_begin.into(), term_begin.into(), READ_ONLY);
        }
        if term_end > 0 {
            if let Some(term_end) = v8::Date::new(scope, (term_end as f64) * 1000.0) {
                obj.define_own_property(scope, v8str_term_end.into(), term_end.into(), READ_ONLY);
            }
        }
    }

    obj
}

fn set_serial<'s>(scope: &mut HandleScope<'s>, object: v8::Local<'s, v8::Object>, serial: u32) {
    if let Some(v8str_last_update) = v8::String::new(scope, "serial") {
        let serial = v8::Integer::new_from_unsigned(scope, serial);
        object.define_own_property(scope, v8str_last_update.into(), serial.into(), READ_ONLY);
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
                READ_ONLY,
            );
        }
    }
}
fn set_uuid<'s>(scope: &mut HandleScope<'s>, object: v8::Local<'s, v8::Object>, uuid: &str) {
    if let Some(v8str_uuid) = v8::String::new(scope, "uuid") {
        let uuid = v8::String::new(scope, uuid).unwrap();
        object.define_own_property(scope, v8str_uuid.into(), uuid.into(), READ_ONLY);
    }
}

pub(super) fn result(
    script: &mut Script,
    worker: &mut MainWorker,
    e: &BytesStart,
    search_map: &HashMap<String, (i32, Vec<Condition>)>,
) {
    let attr = xml_util::attr2hash_map(&e);
    let search = crate::attr_parse_or_static_string(worker, &attr, "search");
    let var = crate::attr_parse_or_static_string(worker, &attr, "var");

    let orders = make_order(&crate::attr_parse_or_static_string(worker, &attr, "sort"));

    let scope = &mut worker.js_runtime.handle_scope();
    let context = scope.get_current_context();
    let scope = &mut v8::ContextScope::new(scope, context);

    let obj = v8::Object::new(scope);

    if search != "" && var != "" {
        if let (
            Some(v8str_var),
            Some(v8str_field),
            Some(v8str_depends),
            Some((collection_id, conditions)),
        ) = (
            v8::String::new(scope, &var),
            v8::String::new(scope, "field"),
            v8::String::new(scope, "depends"),
            search_map.get(&search),
        ) {
            let return_obj = v8::Array::new(scope, 0);

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
                        for r in script
                            .database
                            .clone()
                            .read()
                            .unwrap()
                            .result_session(search, orders)
                            .unwrap()
                        {
                            let (activity, term_begin, term_end) =
                                if let Some(tr) = temporary_collection.get(&r) {
                                    (tr.activity() as i32, tr.term_begin(), tr.term_end())
                                } else if r > 0 {
                                    let row = r as u32;
                                    (
                                        collection.activity(row) as i32,
                                        collection.term_begin(row),
                                        collection.term_begin(row),
                                    )
                                } else {
                                    unreachable!()
                                };

                            let obj = set_values(
                                scope,
                                collection.id(),
                                r,
                                activity,
                                term_begin,
                                term_end,
                            );

                            obj.define_own_property(
                                scope,
                                v8str_session.into(),
                                v8ext_session.into(),
                                READ_ONLY,
                            );

                            obj.define_own_property(
                                scope,
                                v8str_field.into(),
                                v8func_field.into(),
                                READ_ONLY,
                            );
                            obj.define_own_property(
                                scope,
                                v8str_depends.into(),
                                v8func_depends.into(),
                                READ_ONLY,
                            );

                            if r > 0 {
                                set_last_update(scope, obj, collection.last_updated(r as u32));
                                set_uuid(scope, obj, &collection.uuid_str(r as u32));
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
                        let mut search = script.database.clone().read().unwrap().search(collection);
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
                        for r in rows {
                            let obj = set_values(
                                scope,
                                collection_id as i32,
                                r as i64,
                                collection.activity(r) as i32,
                                collection.term_begin(r),
                                collection.term_end(r),
                            );

                            set_serial(scope, obj, collection.serial(r));

                            set_last_update(scope, obj, collection.last_updated(r));
                            set_uuid(scope, obj, &collection.uuid_str(r));

                            obj.define_own_property(
                                scope,
                                v8str_field.into(),
                                v8func_field.into(),
                                READ_ONLY,
                            );
                            obj.define_own_property(
                                scope,
                                v8str_depends.into(),
                                v8func_depends.into(),
                                READ_ONLY,
                            );

                            return_obj.set_index(scope, i, obj.into());
                            i += 1;
                        }
                    }
                }
            }
            obj.set(scope, v8str_var.into(), return_obj.into());
        }
    }
    stack::push(context, scope, obj);
}
