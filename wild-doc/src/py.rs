use std::{
    ffi::CString,
    sync::{Arc, RwLock},
};

use pyo3::{
    pyfunction,
    types::{IntoPyDict, PyCapsule, PyDict, PyModule},
    wrap_pyfunction, PyObject, PyResult, Python,
};
use wild_doc_script::{VarsStack, WildDocScript};

use semilattice_database_session::anyhow::Result;

pub struct WdPy {}
impl WildDocScript for WdPy {
    fn evaluate_module(&mut self, _: &str, src: &[u8]) -> Result<()> {
        self.eval(src)?;
        Ok(())
    }

    fn eval(&mut self, code: &[u8]) -> Result<Option<serde_json::Value>> {
        let obj = Python::with_gil(|py| -> PyResult<PyObject> {
            let locals = [("os", py.import("os")?)].into_py_dict(py);
            let code = std::str::from_utf8(code)?;
            py.eval(code, None, Some(&locals))?.extract()
        })?;
        Ok(Some(obj.to_string().into()))
    }
}

impl WdPy {
    pub fn new(stack: Arc<RwLock<VarsStack>>) -> Self {
        let _ = Python::with_gil(|py| -> PyResult<()> {
            let builtins = PyModule::import(py, "builtins")?;

            let wd = PyModule::new(py, "wd")?;
            wd.add_function(wrap_pyfunction!(wdv, wd)?)?;

            builtins.add_function(wrap_pyfunction!(wdv, builtins)?)?;

            builtins.add_submodule(wd)?;

            let name = CString::new("builtins.wdstack").unwrap();
            let stack = PyCapsule::new(py, stack, Some(name.clone()))?;
            builtins.add("wdstack", stack)?;

            Ok(())
        });
        WdPy {}
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
                .call1((v.value().to_string(),))?
                .extract();
            }
        }
        Ok(PyDict::new(py).into())
    })
}
