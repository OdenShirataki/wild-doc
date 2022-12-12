use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{Arc, RwLock},
};

use deno_runtime::{
    deno_napi::v8::{self, READ_ONLY},
    worker::MainWorker,
};
use quick_xml::events::BytesStart;
use semilattice_database::{Condition, Database, Order, OrderKey, Session};

use crate::xml_util;

use super::Script;

pub fn field(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let this = args.this();

    if let (Some(db), Some(collection_id), Some(row)) = (
        v8::String::new(scope, "wd.db")
            .and_then(|code| v8::Script::compile(scope, code, None))
            .and_then(|v| v.run(scope)),
        v8::String::new(scope, "collection_id")
            .and_then(|s| this.get(scope, s.into()))
            .and_then(|s| s.to_int32(scope)),
        v8::String::new(scope, "row")
            .and_then(|s| this.get(scope, s.into()))
            .and_then(|s| s.to_big_int(scope)),
    ) {
        let db =
            unsafe { v8::Local::<v8::External>::cast(db) }.value() as *mut Arc<RwLock<Database>>;
        let db = unsafe { &mut *db };
        if let Some(field_name) = args.get(0).to_string(scope) {
            let field_name = field_name.to_rust_string_lossy(scope);
            if let Some(data) = db
                .clone()
                .read()
                .unwrap()
                .collection(collection_id.value() as i32)
            {
                if let Some(str) = v8::String::new(
                    scope,
                    std::str::from_utf8(data.field_bytes(row.i64_value().0 as u32, &field_name))
                        .unwrap(),
                ) {
                    rv.set(str.into());
                }
            }
        }
    }
}

pub fn session_field(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let this = args.this();

    if let (Some(db), Some(collection_id), Some(row), Some(session)) = (
        v8::String::new(scope, "wd.db")
            .and_then(|code| v8::Script::compile(scope, code, None))
            .and_then(|v| v.run(scope)),
        v8::String::new(scope, "collection_id")
            .and_then(|s| this.get(scope, s.into()))
            .and_then(|s| s.to_int32(scope)),
        v8::String::new(scope, "row")
            .and_then(|s| this.get(scope, s.into()))
            .and_then(|s| s.to_big_int(scope)),
        v8::String::new(scope, "session").and_then(|s| this.get(scope, s.into())),
    ) {
        let db =
            unsafe { v8::Local::<v8::External>::cast(db) }.value() as *mut Arc<RwLock<Database>>;
        let db = unsafe { &mut *db };

        let session = unsafe { v8::Local::<v8::External>::cast(session) }.value()
            as *mut Arc<RwLock<Session>>;
        let session = unsafe { &mut *session };

        if let Some(field_name) = args.get(0).to_string(scope) {
            let field_name = field_name.to_rust_string_lossy(scope);

            if let Ok(session) = session.clone().read() {
                if let Some(data) = db
                    .clone()
                    .read()
                    .unwrap()
                    .collection(collection_id.value() as i32)
                {
                    let bytes =
                        session.collection_field_bytes(&data, row.i64_value().0, &field_name);
                    if let Some(str) = v8::String::new(scope, std::str::from_utf8(&bytes).unwrap())
                    {
                        rv.set(str.into());
                    }
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

pub(super) fn result(
    script: &Script,
    worker: &mut MainWorker,
    e: &BytesStart,
    search_map: &HashMap<String, (i32, Vec<Condition>)>,
) {
    let attr = xml_util::attr2hash_map(&e);
    let search = crate::attr_parse_or_static(worker, &attr, "search");
    let var = crate::attr_parse_or_static(worker, &attr, "var");

    if search != "" && var != "" {
        if let Some((collection_id, conditions)) = search_map.get(&search) {
            let collection_id = *collection_id;
            let orders = make_order(&crate::attr_parse_or_static(worker, &attr, "sort"));

            let mut session_maybe_has_collection = None;
            for i in (0..script.sessions.len()).rev() {
                if let Some(_) = script.sessions[i]
                    .clone()
                    .read()
                    .unwrap()
                    .temporary_collection(collection_id)
                {
                    session_maybe_has_collection = Some(script.sessions[i].clone());
                    break;
                }
            }

            if let Some(ref session) = session_maybe_has_collection {
                worker.js_runtime.v8_isolate().set_slot(session.clone());
            }

            let scope = &mut worker.js_runtime.handle_scope();
            let context = scope.get_current_context();
            let scope = &mut v8::ContextScope::new(scope, context);

            if let (
                Some(v8str_collection_id),
                Some(v8str_func_field),
                Some(v8str_row),
                Some(v8str_wd),
                Some(v8str_stack),
                Some(v8str_activity),
                Some(v8str_term_begin),
                Some(v8str_term_end),
                Some(v8str_last_update),
                Some(v8str_uuid),
                Some(var),
            ) = (
                v8::String::new(scope, "collection_id"),
                v8::String::new(scope, "field"),
                v8::String::new(scope, "row"),
                v8::String::new(scope, "wd"),
                v8::String::new(scope, "stack"),
                v8::String::new(scope, "activity"),
                v8::String::new(scope, "term_begin"),
                v8::String::new(scope, "term_end"),
                v8::String::new(scope, "last_update"),
                v8::String::new(scope, "uuid"),
                v8::String::new(scope, &var),
            ) {
                let global = context.global(scope);
                if let Some(wd) = global.get(scope, v8str_wd.into()) {
                    if let Ok(wd) = v8::Local::<v8::Object>::try_from(wd) {
                        if let Some(stack) = wd.get(scope, v8str_stack.into()) {
                            if let Ok(stack) = v8::Local::<v8::Array>::try_from(stack) {
                                let obj = v8::Object::new(scope);
                                let return_obj = v8::Array::new(scope, 0);

                                if let Some(mut session) = session_maybe_has_collection {
                                    let addr =
                                        &mut session as *mut Arc<RwLock<Session>> as *mut c_void;
                                    let v8_ext = v8::External::new(scope, addr);
                                    if let (
                                        Some(v8func_field),
                                        Some(v8str_session),
                                        Some(temporary_collection),
                                    ) = (
                                        v8::Function::new(scope, session_field),
                                        v8::String::new(scope, "session"),
                                        session
                                            .clone()
                                            .read()
                                            .unwrap()
                                            .temporary_collection(collection_id),
                                    ) {
                                        let rowset = session
                                            .clone()
                                            .read()
                                            .unwrap()
                                            .search(collection_id, conditions)
                                            .result(&script.database.clone().read().unwrap());
                                        //TODO:セッションデータのソート
                                        let mut i = 0;
                                        for r in rowset {
                                            let obj = v8::Object::new(scope);

                                            obj.define_own_property(
                                                scope,
                                                v8str_session.into(),
                                                v8_ext.into(),
                                                READ_ONLY,
                                            );

                                            let row = v8::BigInt::new_from_i64(scope, r);
                                            obj.set(scope, v8str_row.into(), row.into());
                                            obj.set(
                                                scope,
                                                v8str_func_field.into(),
                                                v8func_field.into(),
                                            );

                                            if let Some(tr) = temporary_collection.get(&r) {
                                                let activity =
                                                    v8::Integer::new(scope, tr.activity() as i32);
                                                let term_begin = v8::BigInt::new_from_i64(
                                                    scope,
                                                    tr.term_begin(),
                                                );
                                                let term_end =
                                                    v8::BigInt::new_from_i64(scope, tr.term_end());

                                                obj.set(
                                                    scope,
                                                    v8str_activity.into(),
                                                    activity.into(),
                                                );
                                                obj.set(
                                                    scope,
                                                    v8str_term_begin.into(),
                                                    term_begin.into(),
                                                );
                                                obj.set(
                                                    scope,
                                                    v8str_term_end.into(),
                                                    term_end.into(),
                                                );
                                            }

                                            let collection_id =
                                                v8::Integer::new(scope, collection_id as i32);
                                            obj.set(
                                                scope,
                                                v8str_collection_id.into(),
                                                collection_id.into(),
                                            );
                                            return_obj.set_index(scope, i, obj.into());
                                            i += 1;
                                        }
                                    }
                                } else {
                                    if let Some(collection) = script
                                        .database
                                        .clone()
                                        .read()
                                        .unwrap()
                                        .collection(collection_id)
                                    {
                                        if let Some(v8func_field) = v8::Function::new(scope, field)
                                        {
                                            let mut search = script
                                                .database
                                                .clone()
                                                .read()
                                                .unwrap()
                                                .search(collection);
                                            for c in conditions {
                                                search = search.search(c.clone());
                                            }

                                            let rowset = script
                                                .database
                                                .clone()
                                                .read()
                                                .unwrap()
                                                .result(&search);
                                            let rows = if orders.len() > 0 {
                                                collection.sort(rowset, orders)
                                            } else {
                                                rowset.into_iter().collect()
                                            };
                                            let mut i = 0;
                                            for r in rows {
                                                let obj = v8::Object::new(scope);

                                                let row = v8::BigInt::new_from_i64(scope, r as i64);
                                                let activity = v8::Integer::new(
                                                    scope,
                                                    collection.activity(r) as i32,
                                                );
                                                let term_begin = v8::BigInt::new_from_i64(
                                                    scope,
                                                    collection.term_begin(r),
                                                );
                                                let term_end = v8::BigInt::new_from_i64(
                                                    scope,
                                                    collection.term_end(r),
                                                );
                                                let last_update = v8::BigInt::new_from_i64(
                                                    scope,
                                                    collection.last_updated(r),
                                                );
                                                let uuid =
                                                    v8::String::new(scope, &collection.uuid_str(r))
                                                        .unwrap();

                                                obj.set(scope, v8str_row.into(), row.into());
                                                obj.set(
                                                    scope,
                                                    v8str_activity.into(),
                                                    activity.into(),
                                                );
                                                obj.set(
                                                    scope,
                                                    v8str_term_begin.into(),
                                                    term_begin.into(),
                                                );
                                                obj.set(
                                                    scope,
                                                    v8str_term_end.into(),
                                                    term_end.into(),
                                                );
                                                obj.set(
                                                    scope,
                                                    v8str_last_update.into(),
                                                    last_update.into(),
                                                );
                                                obj.set(scope, v8str_uuid.into(), uuid.into());

                                                obj.set(
                                                    scope,
                                                    v8str_func_field.into(),
                                                    v8func_field.into(),
                                                );

                                                let collection_id =
                                                    v8::Integer::new(scope, collection_id as i32);
                                                obj.set(
                                                    scope,
                                                    v8str_collection_id.into(),
                                                    collection_id.into(),
                                                );

                                                return_obj.set_index(scope, i, obj.into());
                                                i += 1;
                                            }
                                        }
                                    }
                                }
                                obj.set(scope, var.into(), return_obj.into());
                                stack.set_index(scope, stack.length(), obj.into());
                            }
                        }
                    }
                }
            }
        } else {
            let scope = &mut worker.js_runtime.handle_scope();
            let context = scope.get_current_context();
            let scope = &mut v8::ContextScope::new(scope, context);
            if let (Some(v8str_wd), Some(v8str_stack), Some(var)) = (
                v8::String::new(scope, "wd"),
                v8::String::new(scope, "stack"),
                v8::String::new(scope, &var),
            ) {
                let global = context.global(scope);
                if let Some(wd) = global.get(scope, v8str_wd.into()) {
                    if let Ok(wd) = v8::Local::<v8::Object>::try_from(wd) {
                        if let Some(stack) = wd.get(scope, v8str_stack.into()) {
                            if let Ok(stack) = v8::Local::<v8::Array>::try_from(stack) {
                                let obj = v8::Object::new(scope);
                                let return_obj = v8::Array::new(scope, 0);
                                obj.set(scope, var.into(), return_obj.into());
                                stack.set_index(scope, stack.length(), obj.into());
                            }
                        }
                    }
                }
            }
        }
    }
}
