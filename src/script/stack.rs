use deno_runtime::deno_napi::v8::{self, ContextScope, HandleScope};

pub fn push(
    context: v8::Local<v8::Context>,
    scope: &mut ContextScope<HandleScope>,
    obj: v8::Local<v8::Object>,
) {
    if let (Some(v8str_wd), Some(v8str_stack)) = (
        v8::String::new(scope, "wd"),
        v8::String::new(scope, "stack"),
    ) {
        let global = context.global(scope);
        if let Some(wd) = global.get(scope, v8str_wd.into()) {
            if let Ok(wd) = v8::Local::<v8::Object>::try_from(wd) {
                if let Some(stack) = wd.get(scope, v8str_stack.into()) {
                    if let Ok(stack) = v8::Local::<v8::Array>::try_from(stack) {
                        stack.set_index(scope, stack.length(), obj.into());
                    }
                }
            }
        }
    }
}
