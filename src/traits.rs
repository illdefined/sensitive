use std::io::Error;
use std::ptr::NonNull;

pub trait Pages {
	fn pages(&self) -> Option<NonNull<[u8]>>;
}

pub trait Protectable {
	fn lock(&self) -> Result<(), Error>;
	fn unlock(&self) -> Result<(), Error>;
	fn unlock_mut(&mut self) -> Result<(), Error>;
}
