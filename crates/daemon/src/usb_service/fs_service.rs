use anyhow::Result;
use msla_core::SUPPORTED_FILE_EXTENSIONS;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::{fs, sync::RwLock};
use tracing::info;

type FSType = OnceLock<Arc<RwLock<UsbFilesystem>>>;
static USB_FILESYSTEM: FSType = OnceLock::new();

#[derive(Debug)]
pub struct UsbFilesystem {
    pub available: bool,
    pub file_index: HashMap<u16, Arc<FileEntry>>,
    pub inverted_file_index: HashMap<PathBuf, u16>,

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

    pub file_name: String,

    pub file_type: FileEntryType,
}

impl UsbFilesystem {
    /// Creates new filesystem with provided path and inits store
    pub fn init(root: PathBuf) -> Result<()> {
        let mut hm = HashMap::new();
        hm.insert(
            0,
            Arc::new(FileEntry::new("/", root, 0, 0, FileEntryType::Dir)),
        );

        USB_FILESYSTEM
            .set(Arc::new(RwLock::new(Self {
                file_index: hm,
                available: true,
                _inc: 1,
                inverted_file_index: HashMap::new(),
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
        self.file_index.insert(
            0,
            Arc::new(FileEntry::new("/", root.clone(), 0, 0, FileEntryType::Dir)),
        );

        self.inverted_file_index.clear();
        self.inverted_file_index.insert(root, 0);

        self._inc = 1;
    }

    /// Sets fs availability
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }

    /// Traverses dir by provided id
    pub async fn traverse_dir(&mut self, traverse_id: u16) -> Result<HashMap<u16, Arc<FileEntry>>> {
        let Some(traversing_dir) = self.file_index.get(&traverse_id) else {
            anyhow::bail!("Invalid directory id: index not found");
        };

        let mut hash_map: HashMap<u16, Arc<FileEntry>> = HashMap::new();
        let mut reader = fs::read_dir(&traversing_dir.path).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if let Some(e_i) = self.inverted_file_index.get(&path)
                && let Some(e) = self.file_index.get(e_i)
            {
                // Entry already traversed
                hash_map.insert(*e_i, e.clone());
                continue;
            }

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

            let arc = Arc::new(FileEntry {
                file_name: file_name.to_string_lossy().to_string(),
                path: path.clone(),
                id: self._inc,
                parent_id: traverse_id,
                file_type: t,
            });

            hash_map.insert(self._inc, arc.clone());
            self.file_index.insert(self._inc, arc);
            self.inverted_file_index.insert(path, self._inc);

            self._inc += 1;
        }

        Ok(hash_map)
    }
}

impl FileEntry {
    /// Creates new file entry
    pub fn new(
        name: impl Into<String>,
        path: PathBuf,
        id: u16,
        parent_id: u16,
        t: FileEntryType,
    ) -> Self {
        Self {
            file_name: name.into(),
            id,
            parent_id,
            path,
            file_type: t,
        }
    }
}
