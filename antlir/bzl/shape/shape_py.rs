// @generated SignedSource<<8f2184b7a97ce1bc635ed0172dd3094b>>

use pyo3::prelude::*;

#[pymodule]
/// A Python module implemented in Rust.
fn example(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Character>()?;
    Ok(())
}
#[doc(r######"Groupings in which a character may belong.
"######)]
#[pyclass(subclass)]
struct Affiliations {
    inner: shape::Affiliations,
}

#[pymethods]
impl Affiliations {
    #[getter]
    fn faction(&self) -> PyResult<String> {
        Ok(self.inner.faction.clone().into())
    }
}

impl From<shape::Affiliations> for Affiliations {
    fn from(inner: shape::Affiliations) -> Self {
        Self { inner }
    }
}

impl From<&shape::Affiliations> for Affiliations {
    fn from(inner: &shape::Affiliations) -> Self {
        Self { inner: inner.clone() }
    }
}

#[doc(r######"A character that exists in the Star Wars universe.
Test data adapted from the GraphQL examples
"######)]
#[pyclass(subclass)]
struct Character {
    inner: shape::Character,
}

#[pymethods]
impl Character {
    #[getter]
    fn affiliations(&self) -> PyResult<Affiliations> {
        Ok(self.inner.affiliations.clone().into())
    }
    #[getter]
    fn appears_in(&self) -> PyResult<Vec<i32>> {
        Ok(self.inner.appears_in.clone().into())
    }
    #[getter]
    fn friends(&self) -> PyResult<Vec<Friend>> {
        Ok(self.inner.friends.iter().map(|e| e.clone().into()).collect())
    }
    #[getter]
    fn metadata(&self) -> PyResult<::std::collections::BTreeMap<String, String>> {
        Ok(self.inner.metadata.clone().into())
    }
    #[getter]
    fn name(&self) -> PyResult<String> {
        Ok(self.inner.name.clone().into())
    }
    //#[getter]
    //fn weapon(&self) -> PyResult<Weapon> {
    //    Ok(self.inner.weapon.clone().into())
    //}
}

impl From<shape::Character> for Character {
    fn from(inner: shape::Character) -> Self {
        Self { inner }
    }
}

impl From<&shape::Character> for Character {
    fn from(inner: &shape::Character) -> Self {
        Self { inner: inner.clone() }
    }
}

#[doc(r######"A color that a lightsaber may come in.
"######)]
#[pyclass(subclass)]
struct Color {
    inner: shape::Color,
}

impl From<shape::Color> for Color {
    fn from(inner: shape::Color) -> Self {
        Self { inner }
    }
}

impl From<&shape::Color> for Color {
    fn from(inner: &shape::Color) -> Self {
        Self { inner: inner.clone() }
    }
}

#[pyclass(subclass)]
struct Friend {
    inner: shape::Friend,
}

#[pymethods]
impl Friend {
    #[getter]
    fn name(&self) -> PyResult<String> {
        Ok(self.inner.name.clone().into())
    }
}

impl From<shape::Friend> for Friend {
    fn from(inner: shape::Friend) -> Self {
        Self { inner }
    }
}

impl From<&shape::Friend> for Friend {
    fn from(inner: &shape::Friend) -> Self {
        Self { inner: inner.clone() }
    }
}

#[pyclass(subclass)]
struct Lightsaber {
    inner: shape::Lightsaber,
}

#[pymethods]
impl Lightsaber {
    #[getter]
    fn color(&self) -> PyResult<Color> {
        Ok(self.inner.color.clone().into())
    }
    #[getter]
    fn handmedown(&self) -> PyResult<bool> {
        Ok(self.inner.handmedown.clone().into())
    }
}

impl From<shape::Lightsaber> for Lightsaber {
    fn from(inner: shape::Lightsaber) -> Self {
        Self { inner }
    }
}

impl From<&shape::Lightsaber> for Lightsaber {
    fn from(inner: &shape::Lightsaber) -> Self {
        Self { inner: inner.clone() }
    }
}


