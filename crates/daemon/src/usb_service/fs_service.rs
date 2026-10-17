use anyhow::Result;
use msla_core::SUPPORTED_FILE_EXTENSIONS;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::{fs, sync::RwLock};
use tracing::info;

use crate::uart::packet::UARTPacket;

type FSType = OnceLock<Arc<RwLock<UsbFilesystem>>>;
static USB_FILESYSTEM: FSType = OnceLock::new();

#[derive(Debug)]
pub struct UsbFilesystem {
    pub available: bool,
    pub file_index: HashMap<u16, FileEntry>,

    pub _inc: u16,
}

#[derive(Default, Debug, Clone)]
pub enum FileEntryType {
    #[default]
    File = 0,
    Dir = 1,
}

#[derive(Default, Debug)]
pub struct FileEntry {
    pub id: u16,
    pub parent_id: u16,
    pub path: PathBuf,
    pub file_size: u32,

    pub file_name: String,
    pub file_type: FileEntryType,

    pub children: Option<HashSet<u16>>,
}

impl UsbFilesystem {
    /// Creates new filesystem with provided path and inits store
    pub fn init(root: PathBuf) -> Result<()> {
        let mut hm = HashMap::new();
        hm.insert(0, FileEntry::new_root(root));

        USB_FILESYSTEM
            .set(Arc::new(RwLock::new(Self {
                file_index: hm,
                available: false,
                _inc: 1,
            })))
            .map_err(|_| anyhow::anyhow!("USB filesystem is already initialized"))?;

        Ok(())
    }

    /// Gets already inited db
    pub fn get_filesystem() -> Option<Arc<RwLock<UsbFilesystem>>> {
        USB_FILESYSTEM.get().cloned()
    }

    /// Clears file index and inserts root
    pub fn reinit(&mut self, root: PathBuf) {
        info!("Reinit usb fs to {:?}", root);

        self.file_index.clear();
        self.file_index.insert(0, FileEntry::new_root(root.clone()));

        self._inc = 1;
    }

    /// Sets fs availability
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }

    /// Traverses dir by provided id
    pub async fn dir_contents(&mut self, dir_id: u16) -> Result<(u16, HashSet<u16>)> {
        let Some(dir) = self.file_index.get(&dir_id) else {
            anyhow::bail!("Invalid directory id: index not found: {dir_id}");
        };

        if matches!(dir.file_type, FileEntryType::File) {
            anyhow::bail!("Not a dir");
        }

        if let Some(ref children) = dir.children {
            return Ok((dir.parent_id, children.clone()));
        }

        let parent = dir.parent_id;
        let mut children: HashSet<u16> = HashSet::new();
        let mut reader = fs::read_dir(&dir.path).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            let file_name = entry.file_name();
            let file_type = entry.file_type().await?;

            if file_type.is_file()
                && !path.extension().is_some_and(|ext| {
                    SUPPORTED_FILE_EXTENSIONS.contains(&ext.to_string_lossy().as_ref())
                })
            {
                continue;
            }

            let t = if file_type.is_dir() {
                FileEntryType::Dir
            } else if file_type.is_file() {
                FileEntryType::File
            } else {
                continue;
            };

            let entry = FileEntry {
                file_name: file_name.to_string_lossy().to_string(),
                path: path.clone(),
                id: self._inc,
                parent_id: dir_id,
                file_type: t,
                file_size: entry.metadata().await?.len() as u32,

                children: None,
            };

            children.insert(self._inc);
            self.file_index.insert(self._inc, entry);

            self._inc += 1;
        }

        if let Some(d) = self.file_index.get_mut(&dir_id) {
            d.children = Some(children.clone());
        }

        Ok((parent, children))
    }
}

impl FileEntry {
    /// Creates new root entry
    pub fn new_root(path: PathBuf) -> Self {
        Self {
            file_name: "/".to_string(),
            id: 0,
            parent_id: 0,
            path,
            file_type: FileEntryType::Dir,
            file_size: 0,
            children: None,
        }
    }

    /// Serialize entry to a uart packet
    pub fn serialize_to(&self, packet: &mut UARTPacket) {
        packet.write_u16(self.id); // Entry id
        packet.write_u8(self.file_type.clone() as u8); // File type
        packet.write_u32(self.file_size); // File size
        packet.write_string(self.file_name.clone()); // File name
    }
}
