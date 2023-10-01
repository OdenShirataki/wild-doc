use std::{
    ffi::CString,
    sync::{Arc, RwLock},
};

use pyo3::{
    pyfunction,
    types::{PyCapsule, PyDict, PyModule},
    wrap_pyfunction, PyObject, PyResult, Python,
};
use wild_doc_script::{VarsStack, WildDocScript, WildDocState, WildDocValue};

use wild_doc_script::anyhow::Result;

pub struct WdPy {}
impl WildDocScript for WdPy {
    fn new(state: WildDocState) -> Result<Self> {
        let _ = Python::with_gil(|py| -> PyResult<()> {
            let builtins = PyModule::import(py, "builtins")?;

            let wd = PyModule::new(py, "wd")?;
            wd.add_function(wrap_pyfunction!(wdv, wd)?)?;

            builtins.add_function(wrap_pyfunction!(wdv, builtins)?)?;

            builtins.add_submodule(wd)?;

            let name = CString::new("builtins.wdstack").unwrap();
            let stack = PyCapsule::new(py, state.stack(), Some(name.clone()))?;
            builtins.add("wdstack", stack)?;

            Ok(())
        });
        Ok(WdPy {})
    }
    fn evaluate_module(&self, _: &str, code: &[u8]) -> Result<()> {
        let code = std::str::from_utf8(code)?;
        Python::with_gil(|py| -> PyResult<()> { py.run(code, None, None) })?;
        Ok(())
    }

    fn eval(&self, code: &[u8]) -> Result<WildDocValue> {
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
        let name = CString::new("builtins.wdstack").unwrap();
        let stack: &Arc<RwLock<VarsStack>> = unsafe { PyCapsule::import(py, name.as_ref())? };
        for stack in stack.read().unwrap().iter().rev() {
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
                .call1((v.read().unwrap().to_string(),))?
                .extract();
            }
        }
        Ok(PyDict::new(py).into())
    })
}
