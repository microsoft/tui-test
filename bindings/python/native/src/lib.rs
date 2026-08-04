use pyo3::exceptions::{PyFileNotFoundError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pythonize::{depythonize, pythonize};
use shell_use::runtime::global_registry;

#[pyclass(module = "shell_use._native", frozen)]
struct NativeSession {
    name: String,
}

#[pymethods]
impl NativeSession {
    #[new]
    fn new(name: String) -> Self {
        NativeSession { name }
    }

    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    fn request<'py>(&self, py: Python<'py>, payload: Bound<'py, PyAny>) -> PyResult<Py<PyAny>> {
        let request: serde_json::Value =
            depythonize(&payload).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let name = self.name.clone();
        let response = py.detach(move || global_registry().response_value(&name, request));
        let response = serde_json::to_value(&response)
            .map_err(|e| PyRuntimeError::new_err(format!("failed to encode response: {e}")))?;
        Ok(pythonize(py, &response)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
            .unbind())
    }

    fn recording(&self, py: Python<'_>) -> PyResult<String> {
        let name = self.name.clone();
        py.detach(move || global_registry().recording(&name))
            .map_err(|e| io_error_to_py(&e))
    }
}

#[pyfunction]
fn sessions(py: Python<'_>) -> Vec<String> {
    py.detach(|| global_registry().sessions())
}

#[pyfunction]
fn close_all(py: Python<'_>) {
    py.detach(|| global_registry().close_all());
}

#[pyfunction]
fn recording(py: Python<'_>, name: String) -> PyResult<String> {
    py.detach(move || global_registry().recording(&name))
        .map_err(|e| io_error_to_py(&e))
}

fn io_error_to_py(error: &std::io::Error) -> PyErr {
    if error.kind() == std::io::ErrorKind::NotFound {
        PyFileNotFoundError::new_err(error.to_string())
    } else {
        PyRuntimeError::new_err(error.to_string())
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NativeSession>()?;
    m.add_function(wrap_pyfunction!(sessions, m)?)?;
    m.add_function(wrap_pyfunction!(close_all, m)?)?;
    m.add_function(wrap_pyfunction!(recording, m)?)?;
    Ok(())
}
