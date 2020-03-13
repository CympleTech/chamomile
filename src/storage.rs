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

    pub fn read<T: Serialize + DeserializeOwned>(&self, k: &Vec<u8>) -> Option<T> {
        self.tree
            .get(k)
            .ok()
            .map(|v| v)
            .flatten()
            .map(|v| bincode::deserialize(&v).ok())
            .flatten()
    }

    pub fn write<T: Serialize + DeserializeOwned>(&self, k: Vec<u8>, t: &T) -> Result<(), Error> {
        bincode::serialize(&t)
            .map_err(|_e| new_error("db serialize error!"))
            .and_then(|bytes| {
                self.tree
                    .insert(k, bytes)
                    .map_err(|_e| new_error("db write failure!"))
                    .and_then(|_| self.flush())
            })
    }

    pub fn update<T: Serialize + DeserializeOwned>(&self, k: Vec<u8>, t: &T) -> Result<(), Error> {
        bincode::serialize(&t)
            .map_err(|_e| new_error("db serialize error!"))
            .and_then(|bytes| {
                let old = self.tree.get(&k).ok().map(|v| v).flatten();
                if old.is_none() {
                    self.tree
                        .insert(k, bytes)
                        .map_err(|_e| new_error("db write failure!"))
                        .and_then(|_| self.flush())
                } else {
                    self.tree
                        .compare_and_swap(k, Some(old.unwrap()), Some(bytes))
                        .map_err(|_e| new_error("db swap failure!"))
                        .and_then(|_| self.flush())
                }
            })
    }

    pub fn delete<T: Serialize + DeserializeOwned>(&self, k: &Vec<u8>) -> Result<T, Error> {
        let result = self.read::<T>(k);
        if result.is_some() {
            self.tree
                .remove(k)
                .map_err(|_e| new_error("db delete error"))
                .and_then(|_| self.flush())?;
            Ok(result.unwrap())
        } else {
            Err(new_error("db delete key not found!"))
        }
    }

    fn flush(&self) -> Result<(), Error> {
        async_std::task::spawn(self.tree.flush_async());
        Ok(())
    }
}
