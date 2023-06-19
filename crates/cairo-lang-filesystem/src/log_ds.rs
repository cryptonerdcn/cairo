use std::sync::Arc;
use std::sync::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref DB: Mutex<LogDatabase> = Mutex::new(LogDatabase::default());
}

#[salsa::query_group(FileTextsStorage)]
pub trait FileTexts: salsa::Database {
    #[salsa::input]
    fn file_text(&self, file: String) -> Arc<String>;
}

#[derive(Default)]
#[salsa::database(FileTextsStorage)]
pub struct LogDatabase {
    storage: salsa::Storage<LogDatabase>,
}

impl salsa::Database for LogDatabase {}
impl  LogDatabase {
    pub fn create_file_text(file: String, text: String) {
        let mut db = DB.lock().unwrap();
        db.set_file_text(file, Arc::new(text));
    }

    pub fn append_file_text(file: String, text: String) {
        let mut db = DB.lock().unwrap();
        let current_text = db.file_text(file.clone());
        let new_text = format!("{}{}", &*current_text, text);
        db.set_file_text(file, Arc::new(new_text));
    }

    pub fn get_file_text(file: String) -> String {
        let db = DB.lock().unwrap();
        let text = db.file_text(file);
        return text.to_string();
    }
}