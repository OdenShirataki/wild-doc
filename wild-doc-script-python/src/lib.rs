use std::{ffi::CString, sync::Arc};

use pyo3::{
    pyfunction,
    types::{PyCapsule, PyDict, PyModule},
    wrap_pyfunction, PyObject, PyResult, Python,
};
use wild_doc_script::{anyhow::Result, async_trait, WildDocScript, WildDocState, WildDocValue};

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

    async fn evaluate_module(&mut self, _: &str, code: &[u8]) -> Result<()> {
        let code = std::str::from_utf8(code)?;
        Python::with_gil(|py| -> PyResult<()> { py.run(code, None, None) })?;
        Ok(())
    }

    async fn eval(&mut self, code: &[u8]) -> Result<Arc<WildDocValue>> {
        Ok(Arc::new(WildDocValue::Binary(
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
        )))
    }
}

#[pyfunction]
#[pyo3(name = "v")]
fn wdv(_py: Python, key: String) -> PyResult<PyObject> {
    Python::with_gil(|py| -> PyResult<PyObject> {
        let state: &Arc<WildDocState> =
            unsafe { PyCapsule::import(py, CString::new("builtins.wdstate")?.as_ref())? };

        for stack in state.stack().lock().iter().rev() {
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
        }

        Ok(PyDict::new(py).into())
    })
}
