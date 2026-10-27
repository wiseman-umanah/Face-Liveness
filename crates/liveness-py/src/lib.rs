use pyo3::prelude::*;

#[pyfunction]
fn version() -> &'static str {
    liveness_core::version()
}

#[pymodule]
fn liveness_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)
}
