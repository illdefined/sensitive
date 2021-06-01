use crate::pages::Pages;

use std::io::Error;

pub trait AsPages {
	fn as_pages(&self) -> Option<Pages>;
}

pub trait Protectable {
	fn lock(&self) -> Result<(), Error>;
	fn unlock(&self) -> Result<(), Error>;
	fn unlock_mut(&mut self) -> Result<(), Error>;
}
