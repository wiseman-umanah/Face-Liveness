use pyo3::prelude::*;

#[pyfunction]
fn version() -> &'static str {
    liveness_core::version()
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)
}
