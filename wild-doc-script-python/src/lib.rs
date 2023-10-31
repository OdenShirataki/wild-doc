use std::{ffi::CString, sync::Arc};

use pyo3::{
    pyfunction,
    types::{PyCapsule, PyDict, PyModule},
    wrap_pyfunction, PyObject, PyResult, Python,
};
use wild_doc_script::{
    anyhow::Result, async_trait, Vars, WildDocScript, WildDocState, WildDocValue,
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

            builtins.add(
                "wdstate",
                PyCapsule::new(py, state, Some(CString::new("builtins.wdstate")?))?,
            )?;

            Ok(())
        });
        Ok(WdPy {})
    }

    async fn evaluate_module(&mut self, _: &str, code: &str, stack: &Vars) -> Result<()> {
        Python::with_gil(|py| -> PyResult<()> {
            let builtins = PyModule::import(py, "builtins")?;
            builtins.set_item(
                "wdstack",
                PyCapsule::new(py, stack.clone(), Some(CString::new("builtins.wdstack")?))?,
            )?;

            py.run(code, None, None)
        })?;
        Ok(())
    }

    async fn eval(&mut self, code: &str, stack: &Vars) -> Result<Arc<WildDocValue>> {
        Ok(Arc::new(WildDocValue::Binary(
            Python::with_gil(|py| -> PyResult<PyObject> {
                let builtins = PyModule::import(py, "builtins")?;
                builtins.set_item(
                    "wdstack",
                    PyCapsule::new(py, stack.clone(), Some(CString::new("builtins.wdstack")?))?,
                )?;
                py.eval(("(".to_owned() + code + ")").as_str(), None, None)?
                    .extract()
            })?
            .to_string()
            .into_bytes(),
        )))
    }
}

#[pyfunction]
#[pyo3(name = "v")]
fn wdv(_py: Python, key: String) -> PyResult<PyObject> {
    Python::with_gil(|py| -> PyResult<PyObject> {
        let stack: &Vars =
            unsafe { PyCapsule::import(py, CString::new("builtins.wdstack")?.as_ref())? };

        if let Some(v) = stack.get(&key) {
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

        Ok(PyDict::new(py).into())
    })
}
