extern crate vint;
extern crate serde;

// The serde_derive crate provides the macros for #[derive(Serialize)] and
// #[derive(Deserialize)]. You won't need these for implementing a data format
// but your unit tests will probably use them - hence #[cfg(test)].
// #[cfg(test)]
// #[macro_use]
// extern crate serde_derive;

// mod de;
mod error;
mod ser;
mod de;

// pub use de::{from_str, Deserializer};
pub use self::error::{Error, Result};
// pub use ser::{to_string, Serializer};



// mod ser;

// #[cfg(test)]
// mod tests {
//     #[test]
//     fn it_works() {
//         assert_eq!(2 + 2, 4);
//     }
// }
