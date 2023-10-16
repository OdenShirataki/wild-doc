use std::{ffi::CString, ops::Deref, sync::Arc};

use indexmap::IndexMap;
use parking_lot::Mutex;
use pyo3::{
    pyfunction,
    types::{PyCapsule, PyDict, PyModule},
    wrap_pyfunction, PyObject, PyResult, Python,
};
use wild_doc_script::{
    anyhow::Result, async_trait, VarsStack, WildDocScript, WildDocState, WildDocValue,
};

pub struct WdPy {}


#[async_trait(?Send)]
impl WildDocScript for WdPy {
    fn new(state: Arc<WildDocState>) -> Result<Self> {
        let _ = Python::with_gil(|py| -> PyResult<()> {
            let builtins = PyModule::import(py, "builtins")?;

            let wd = PyModule::new(py, "wd")?;
            wd.add_function(wrap_pyfunction!(wdv, wd)?)?;

            builtins.add_function(wrap_pyfunction!(wdv, builtins)?)?;

            builtins.add_submodule(wd)?;

            let name = CString::new("builtins.wdstack").unwrap();
            let stack = PyCapsule::new(py, Arc::clone(state.stack()), Some(name))?;
            builtins.add("wdstack", stack)?;

            let name = CString::new("builtins.wdglobal").unwrap();
            let global = PyCapsule::new(py, Arc::clone(state.global()), Some(name))?;
            builtins.add("wdglobal", global)?;

            Ok(())
        });
        Ok(WdPy {})
    }

    async fn evaluate_module(&self, _: &str, code: &[u8]) -> Result<()> {
        let code = std::str::from_utf8(code)?;
        Python::with_gil(|py| -> PyResult<()> { py.run(code, None, None) })?;
        Ok(())
    }

    async fn eval(&self, code: &[u8]) -> Result<WildDocValue> {
        Ok(WildDocValue::Binary(
            Python::with_gil(|py| -> PyResult<PyObject> {
                py.eval(
                    ("(".to_owned() + std::str::from_utf8(code)? + ")").as_str(),
                    None,
                    None,
                )?
                .extract()
            })?
            .to_string()
            .into_bytes(),
        ))
    }
}

#[pyfunction]
#[pyo3(name = "v")]
fn wdv(_py: Python, key: String) -> PyResult<PyObject> {
    Python::with_gil(|py| -> PyResult<PyObject> {
        if key == "global" {
            let global: &Arc<Mutex<IndexMap<String, WildDocValue>>> =
                unsafe { PyCapsule::import(py, CString::new("builtins.wdglobal")?.as_ref())? };

            return PyModule::from_code(
                py,
                r#"
            import json

            def v(data):
            return json.loads(data)
            "#,
                "",
                "",
            )?
            .getattr("v")?
            .call1((WildDocValue::Object(global.lock().deref().clone()).to_string(),))?
            .extract();
        } else {
            let stack: &Arc<Mutex<VarsStack>> =
                unsafe { PyCapsule::import(py, CString::new("builtins.wdstack")?.as_ref())? };

            for stack in stack.lock().iter().rev() {
                if let Some(v) = stack.get(key.as_bytes()) {
                    return PyModule::from_code(
                        py,
                        r#"
import json

def v(data):
    return json.loads(data)
"#,
                        "",
                        "",
                    )?
                    .getattr("v")?
                    .call1((v.to_string(),))?
                    .extract();
                }
            }
        }

        Ok(PyDict::new(py).into())
    })
}
