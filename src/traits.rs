use std::io::Error;

pub trait Protectable {
	fn lock(&self) -> Result<(), Error>;
	fn unlock(&self) -> Result<(), Error>;
	fn unlock_mut(&mut self) -> Result<(), Error>;
}
