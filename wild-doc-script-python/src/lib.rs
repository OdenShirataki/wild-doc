use std::{ffi::CString, path::PathBuf, sync::Arc};

use parking_lot::Mutex;
use pyo3::{
    prelude::*,
    types::{PyCapsule, PyDict},
    wrap_pyfunction,
};
use wild_doc_script::{
    IncludeAdaptor, Stack, WildDocScript, WildDocValue, anyhow::Result, async_trait, serde_json,
};

const STACK_CAPSULE_NAME: &std::ffi::CStr = c"builtins.wdstack";

pub struct WdPy {}

/// Puts the current stack into builtins, so that `v()` can read it.
fn set_stack(py: Python<'_>, stack: &Stack) -> PyResult<()> {
    let builtins = PyModule::import(py, "builtins")?;
    // PyCapsule::import("builtins.wdstack") reads the attribute "wdstack" of builtins.
    builtins.setattr(
        "wdstack",
        PyCapsule::new_with_value(py, stack.clone(), STACK_CAPSULE_NAME)?,
    )
}

#[async_trait(?Send)]
impl<I: IncludeAdaptor + Send> WildDocScript<I> for WdPy {
    fn new(_: Arc<Mutex<I>>, _: PathBuf, _: &Stack) -> Result<Self> {
        let _ = Python::attach(|py| -> PyResult<()> {
            let builtins = PyModule::import(py, "builtins")?;

            let wd = PyModule::new(py, "wd")?;
            wd.add_function(wrap_pyfunction!(wdv, &wd)?)?;

            builtins.add_function(wrap_pyfunction!(wdv, &builtins)?)?;

            builtins.add_submodule(&wd)?;

            Ok(())
        });
        Ok(WdPy {})
    }

    async fn evaluate_module(&mut self, _: &str, code: &str, stack: &Stack) -> Result<()> {
        let code = CString::new(code)?;
        Python::attach(|py| -> PyResult<()> {
            set_stack(py, stack)?;
            py.run(&code, None, None)
        })?;
        Ok(())
    }

    async fn eval(&mut self, code: &str, stack: &Stack) -> Result<WildDocValue> {
        let code = CString::new(code)?;
        let value = Python::attach(|py| -> PyResult<String> {
            set_stack(py, stack)?;
            Ok(py.eval(&code, None, None)?.str()?.to_string())
        })?;
        Ok(WildDocValue::Binary(value.into_bytes()))
    }
}

#[pyfunction]
#[pyo3(name = "v")]
fn wdv(py: Python<'_>, key: String) -> PyResult<Py<PyAny>> {
    let stack: &Stack = unsafe { PyCapsule::import(py, STACK_CAPSULE_NAME)? };

    if let Some(v) = stack.get(&Arc::new(key)) {
        let json = serde_json::to_string(v)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        return Ok(PyModule::import(py, "json")?
            .getattr("loads")?
            .call1((json,))?
            .unbind());
    }

    Ok(PyDict::new(py).into_any().unbind())
}
