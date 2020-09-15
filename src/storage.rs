use postcard::{from_bytes, to_allocvec};
use serde::{de::DeserializeOwned, Serialize};
use std::io::{Error, ErrorKind};
use std::path::PathBuf;

fn new_error(s: &str) -> Error {
    Error::new(ErrorKind::Other, s)
}

pub struct LocalDB {
    tree: sled::Db,
}

impl LocalDB {
    pub fn open_absolute(path: &PathBuf) -> Result<LocalDB, Error> {
        let tree = sled::open(path).map_err(|_e| new_error("db open failure!"))?;
        Ok(LocalDB { tree })
    }

    pub fn read<T: Serialize + DeserializeOwned>(&self, k: &[u8]) -> Option<T> {
        self.tree
            .get(k)
            .ok()
            .map(|v| v)
            .flatten()
            .map(|v| from_bytes(&v).ok())
            .flatten()
    }

    pub fn write<T: Serialize + DeserializeOwned>(&self, k: Vec<u8>, t: &T) -> Result<(), Error> {
        to_allocvec(t)
            .map_err(|_e| new_error("db serialize error!"))
            .and_then(|bytes| {
                self.tree
                    .insert(k, bytes)
                    .map_err(|_e| new_error("db write failure!"))
                    .and_then(|_| {
                        self.tree.flush()?;
                        Ok(())
                    })
            })
    }

    pub fn update<T: Serialize + DeserializeOwned>(&self, k: Vec<u8>, t: &T) -> Result<(), Error> {
        to_allocvec(&t)
            .map_err(|_e| new_error("db serialize error!"))
            .and_then(|bytes| {
                let old = self.tree.get(&k).ok().map(|v| v).flatten();
                if old.is_none() {
                    self.tree
                        .insert(k, bytes)
                        .map_err(|_e| new_error("db write failure!"))
                        .and_then(|_| {
                            self.tree.flush()?;
                            Ok(())
                        })
                } else {
                    self.tree
                        .compare_and_swap(k, Some(old.unwrap()), Some(bytes))
                        .map_err(|_e| new_error("db swap failure!"))
                        .and_then(|_| {
                            self.tree.flush()?;
                            Ok(())
                        })
                }
            })
    }

    pub fn delete<T: Serialize + DeserializeOwned>(&self, k: &[u8]) -> Result<T, Error> {
        let result = self.read::<T>(k);
        if result.is_some() {
            self.tree
                .remove(k)
                .map_err(|_e| new_error("db delete error"))
                .and_then(|_| {
                    self.tree.flush()?;
                    Ok(())
                })?;
            Ok(result.unwrap())
        } else {
            Err(new_error("db delete key not found!"))
        }
    }

    fn flush(&self) -> Result<(), Error> {
        //smol::spawn(self.tree.flush_async());
        //Ok(())
        todo!();
    }
}
