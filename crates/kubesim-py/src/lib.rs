//! KubeSim Python — PyO3 bindings for single run and batch execution.

use pyo3::prelude::*;

#[pymodule]
fn kubesim_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
