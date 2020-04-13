use crate::Document;
use crate::Row;
use crate::Terminal;


extern crate clipboard;

use clipboard::ClipboardProvider;
use clipboard::ClipboardContext;

use std::env;
use std::fs;

use std::time::{Duration, SystemTime, Instant};
use termion::color;
use termion::event::Key;
use std::io::{Error, Write};

const STATUS_FG_COLOR: color::Rgb = color::Rgb(63, 63, 63);
const STATUS_BG_COLOR: color::Rgb = color::Rgb(239, 239, 239); //const STATUS_BG_COLOR: color::Rgb = color::Rgb(39, 40, 34);
const BG_COLOR: color::Rgb = color::Rgb(239, 239, 239); //const BG_COLOR: color::Rgb = color::Rgb(39, 40, 34);
const VERSION: &str = env!("CARGO_PKG_VERSION");
const QUIT_TIMES: u8 = 3;
const BACKUP_AT:u32 = 10;
const CACHE_FILE:&str="tmp";

const SET_BG:bool = false;


#[derive(PartialEq, Copy, Clone)]
pub enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Default, Clone)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

struct StatusMessage {
    text: String,
    time: Instant,
}
impl StatusMessage {
    fn from(message: String) -> Self {
        Self {
            time: Instant::now(),
            text: message,
        }
    }
}

pub struct Editor {
    should_quit: bool,
    terminal: Terminal,
    cursor_position: Position,
    offset: Position,
    document: Document,
    status_message: StatusMessage,
    quit_times: u8,
    highlighted_word: Option<String>,
    since_last_backup:u32,
    editor_cache:String,
    init_time:SystemTime,
}

impl Editor {
    pub fn run(&mut self) {
        loop {
            if let Err(error) = self.refresh_ui() {
                self.backup();
                die(error);
            }
            let keep_alive = self.process_keypress();
            match keep_alive {
                Ok(true) => {},
                Err(e) => { die(e); },
                Ok(false) => break
            }  
        }
        Terminal::clear_screen();
        print!("\n\n");
    }
    pub fn default() -> Self {
        let args: Vec<String> = env::args().collect();
        let mut initial_status = String::from("ctrl-f:'find' | ctrl-s:'save' | ctrl-q:'quit' | ctrl-b:'temporary backup'");

        let document = if let Some(file_name) = args.get(1) {
            let doc = Document::open(file_name);
            if let Ok(doc) = doc {
                doc
            } else {
                initial_status = format!("__err__: cannot open: {}", file_name);
                Document::default()
            }
        } else {
            Document::default()
        };

        Self {
            should_quit: false,
            terminal: Terminal::default().expect("__could_not_initialize_terminal__"),
            document,
            cursor_position: Position::default(),
            offset: Position::default(),
            status_message: StatusMessage::from(initial_status),
            quit_times: QUIT_TIMES,
            highlighted_word: None,
            since_last_backup: 0,
            editor_cache: CACHE_FILE.to_string(),
            init_time: SystemTime::now(), 
        }
    }


    fn refresh_ui(&mut self) -> Result<(), std::io::Error> {
        Terminal::cursor_hide();
        Terminal::cursor_position(&Position::default());
        self.document.highlight(
            &self.highlighted_word,
            Some(self.offset.y.saturating_add(self.terminal.size().height as usize),),
        );
        self.draw_rows();
        self.draw_status_bar();
        self.draw_message_bar();
        Terminal::cursor_position(&Position {
            x: self.cursor_position.x.saturating_sub(self.offset.x),
            y: self.cursor_position.y.saturating_sub(self.offset.y),
        });
        
        Terminal::cursor_show();
        Terminal::flush()
    }
    fn save(&mut self) {
        if self.document.file_name.is_none() {
            let new_name = self.prompt("__save_as__: ", |_, _, _| {}).unwrap_or(None);
            if new_name.is_none() {
                self.status("__save_aborted__", true);
                return;
            }
            self.document.file_name = new_name;
        }

        if self.document.save().is_ok() {
            self.status("__save_ok__", false);
        } else {
            self.status("__error_saving__", false);
        }
    }
    fn paste(&mut self){
        let mut ctx: ClipboardContext = ClipboardProvider::new().unwrap();
        let data = ctx.get_contents().unwrap();
        self.status("__pasted__", false);
        for e in data.chars().rev() {
            self.document.insert(&self.cursor_position, e);
        }
    }
    fn backup(&mut self){
        let doc = &self.document;
        let cache_file = if let Some(fname) = &doc.file_name {
            format!("{}.tmp", fname)
        } else {
            self.editor_cache.clone()
        };
        let mut file = fs::File::create(&cache_file).unwrap();
        let lines = doc.doc_read();
        let mut all_ok = true;
        for line in lines  {
            if let Ok(_) =  file.write_all(line.as_bytes()){
              file.write_all(b"\n");
            } else {
              all_ok = false;
            }
        }
        if all_ok {
            self.status(&format!("__backed_up__:{}", &cache_file), false);
            self.since_last_backup = 0;
        }
    }

    fn command_mode(&mut self){
        let mut command = String::from("::");
        loop {
            if let Ok(k) = Terminal::read_key() {
                match k {
                    Key::Char(c) => { 
                        self.status(&command, true);
                        command.push(c);
                
                    },
                    _ => break
                }
            }
        }
    }
    
    fn status(&mut self, msg:&str, refresh:bool){
        if let Ok(elapsed) = self.init_time.elapsed() {
            self.status_message = StatusMessage::from(format!("[{}] {}", elapsed.as_secs(), msg));
        } else {
            self.status_message = StatusMessage::from(format!("{}", msg))
        }
        if(refresh){ self.refresh_ui(); }
    }


    fn search(&mut self) {
        let old_position = self.cursor_position.clone();
        let mut direction = SearchDirection::Forward;
        let query = self
            .prompt(
                "search (esc:cancel, arrows:navigate): ",
                |editor, key, query| {
                    let mut moved = false;
                    match key {
                        Key::Right | Key::Down => {
                            direction = SearchDirection::Forward;
                            editor.move_cursor(Key::Right);
                            moved = true;
                        }
                        Key::Left | Key::Up => direction = SearchDirection::Backward,
                        _ => direction = SearchDirection::Forward,
                    }
                    if let Some(position) =
                        editor
                            .document
                            .find(&query, &editor.cursor_position, direction)
                    {
                        editor.cursor_position = position;
                        editor.scroll();
                    } else if moved {
                        editor.move_cursor(Key::Left);
                    }
                    editor.highlighted_word = Some(query.to_string());
                },
            )
            .unwrap_or(None);

        if query.is_none() {
            self.cursor_position = old_position;
            self.scroll();
        }
        self.highlighted_word = None;
    }
    fn process_keypress(&mut self) -> Result<bool, std::io::Error> {
        let pressed_key = Terminal::read_key()?;
        self.since_last_backup += 1;
        if self.since_last_backup >= BACKUP_AT {
            self.backup();
        }
        match pressed_key {
            Key::Ctrl('q') => {
                if self.quit_times > 0 && self.document.is_dirty() {
                    self.status(&format!("__warn__:unsaved changes. hit ctrl-q {} more times to quit.", self.quit_times), true);
                    self.quit_times -= 1;
                    return Ok(self.quit_times > 0);
                }
                self.should_quit = true
            }
            Key::Ctrl('s') => self.save(),
            Key::Ctrl('f') => self.search(),
            Key::Ctrl('b') => self.backup(),
            Key::Ctrl('v') => self.paste(),
            Key::Ctrl('x') => self.command_mode(),
            Key::Char(c) => {
                self.document.insert(&self.cursor_position, c);
                self.move_cursor(Key::Right);
            }
            Key::Delete => self.document.delete(&self.cursor_position),
            Key::Backspace => {
                if self.cursor_position.x > 0 || self.cursor_position.y > 0 {
                    self.move_cursor(Key::Left);
                    self.document.delete(&self.cursor_position);
                }
            }
            Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::PageUp
            | Key::PageDown
            | Key::End
            | Key::Home => self.move_cursor(pressed_key),
            _ => (),
        }
        self.scroll();
        if self.quit_times < QUIT_TIMES {
            self.quit_times = QUIT_TIMES;
            self.status_message = StatusMessage::from(String::new());
        }
        Ok(true)
    }
    fn scroll(&mut self) {
        let Position { x, y } = self.cursor_position;
        let width = self.terminal.size().width as usize;
        let height = self.terminal.size().height as usize;
        let mut offset = &mut self.offset;
        if y < offset.y {
            offset.y = y;
        } else if y >= offset.y.saturating_add(height) {
            offset.y = y.saturating_sub(height).saturating_add(1);
        }
        if x < offset.x {
            offset.x = x;
        } else if x >= offset.x.saturating_add(width) {
            offset.x = x.saturating_sub(width).saturating_add(1);
        }
    }
    fn move_cursor(&mut self, key: Key) {
        let terminal_height = self.terminal.size().height as usize;
        let Position { mut y, mut x } = self.cursor_position;
        let height = self.document.len();
        let mut width = if let Some(row) = self.document.row(y) {
            row.len()
        } else {
            0
        };
        match key {
            Key::Up => y = y.saturating_sub(1),
            Key::Down => {
                if y < height {
                    y = y.saturating_add(1);
                }
            }
            Key::Left => {
                if x > 0 {
                    x -= 1;
                } else if y > 0 {
                    y -= 1;
                    if let Some(row) = self.document.row(y) {
                        x = row.len();
                    } else {
                        x = 0;
                    }
                }
            }
            Key::Right => {
                if x < width {
                    x += 1;
                } else if y < height {
                    y += 1;
                    x = 0;
                }
            }
            Key::PageUp => {
                y = if y > terminal_height {
                    y.saturating_sub(terminal_height)
                } else {
                    0
                }
            }
            Key::PageDown => {
                y = if y.saturating_add(terminal_height) < height {
                    y.saturating_add(terminal_height)
                } else {
                    height
                }
            }
            Key::Home => x = 0,
            Key::End => x = width,
            _ => (),
        }
        width = if let Some(row) = self.document.row(y) {
            row.len()
        } else {
            0
        };
        if x > width {
            x = width;
        }

        self.cursor_position = Position { x, y }
    }
    fn draw_welcome_message(&self) {
        let mut welcome_message = format!("editrs v{}", VERSION);
        let width = self.terminal.size().width as usize;
        let len = welcome_message.len();
        #[allow(clippy::integer_arithmetic, clippy::integer_division)]
        let padding = width.saturating_sub(len) / 2;
        let spaces = " ".repeat(padding.saturating_sub(1));
        welcome_message = format!("~{}{}", spaces, welcome_message);
        welcome_message.truncate(width);
        println!("{}\r", welcome_message);
    }
    pub fn draw_row(&self, row: &Row) {
        let width = self.terminal.size().width as usize;
        let start = self.offset.x;
        let end = self.offset.x.saturating_add(width);
        let row = row.render(start, end);
        println!("{}\r", row)
    }
    #[allow(clippy::integer_division, clippy::integer_arithmetic)]
    fn draw_rows(&self) {
        let height = self.terminal.size().height;
        for terminal_row in 0..height {
            //Terminal::set_bg_color(BG_COLOR);
            Terminal::clear_current_line();
            if let Some(row) = self
                .document
                .row(self.offset.y.saturating_add(terminal_row as usize))
            {
                self.draw_row(row);
            } else if self.document.is_empty() && terminal_row == height / 3 {
                self.draw_welcome_message();
            } else {
                println!("~\r");
            }
        }
        
    }
    fn draw_status_bar(&self) {
        let mut status;
        let width = self.terminal.size().width as usize;
        let modified_indicator = if self.document.is_dirty() {
            " (modified)"
        } else {
            ""
        };

        let mut file_name = "[No Name]".to_string();
        if let Some(name) = &self.document.file_name {
            file_name = name.clone();
            file_name.truncate(20);
        }
        status = format!(
            "{} - {} lines{}",
            file_name,
            self.document.len(),
            modified_indicator
        );

        let line_indicator = format!(
            "{} | {}/{}",
            self.document.file_type(),
            self.cursor_position.y.saturating_add(1),
            self.document.len()
        );
        #[allow(clippy::integer_arithmetic)]
        let len = status.len() + line_indicator.len();
        status.push_str(&" ".repeat(width.saturating_sub(len)));
        status = format!("{}{}", status, line_indicator);
        status.truncate(width);
        //Terminal::set_bg_color(BG_COLOR);
        Terminal::set_fg_color(STATUS_FG_COLOR);
        println!("{}\r", status);
        Terminal::reset_fg_color();
        Terminal::reset_bg_color();;
    }
    fn draw_message_bar(&self) {
        //Terminal::set_bg_color(BG_COLOR);
        Terminal::clear_current_line();
        let message = &self.status_message;
        if Instant::now() - message.time < Duration::new(5, 0) {
            let mut text = message.text.clone();
            text.truncate(self.terminal.size().width as usize);
            print!("{}", text);
        }
        //for e in 1..5 {
        //   Terminal::set_bg_color(BG_COLOR);
        //    Terminal::clear_current_line();
        //    println!("...");
        //}
    }
    fn prompt<C>(&mut self, prompt: &str, mut callback: C) -> Result<Option<String>, std::io::Error>
    where
        C: FnMut(&mut Self, Key, &String),
    {
        let mut result = String::new();
        loop {
            self.status(&format!("{}{}", prompt, result), true);
            let key = Terminal::read_key()?;
            match key {
                Key::Backspace => result.truncate(result.len().saturating_sub(1)),
                Key::Char('\n') => break,
                Key::Char(c) => {
                    if !c.is_control() {
                        result.push(c);
                    }
                }
                Key::Esc => {
                    result.truncate(0);
                    break;
                }
                _ => (),
            }
            callback(self, key, &result);
        }
        self.status("", false);
        if result.is_empty() {
            return Ok(None);
        }
        Ok(Some(result))
    }
}

fn die(e: std::io::Error) {
    Terminal::clear_screen();
    panic!(e);
}
