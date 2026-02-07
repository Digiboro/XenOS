//! Menuconfig TUI implementation using ratatui
//!
//! Provides an interactive terminal interface for configuring XenOS build options.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::Alignment,
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::io::stdout;

use super::loader::{Config, Kconfig};
use crate::util::Paths;

/// Dialog type for popups
#[derive(Clone, PartialEq)]
enum DialogType {
    None,
    SaveConfirm,
    SavedNotification,
    ExitConfirm,
    StringInput { id: String, prompt: String },
}

/// State for the menuconfig TUI
pub struct MenuConfigApp {
    kconfig: Kconfig,
    config: Config,
    paths: Paths,

    // Navigation state
    current_menu_stack: Vec<usize>, // Stack of menu indices for navigation
    list_state: ListState,
    show_help: bool,

    // Dialog state
    dialog: DialogType,
    notification_timer: u8,
    input_buffer: String,

    // Should exit flag
    should_exit: bool,
    config_modified: bool,
}

impl MenuConfigApp {
    pub fn new() -> Result<Self> {
        let paths = Paths::new()?;
        let kconfig = Kconfig::load(&paths.kconfig_file)?;
        let config = Config::load_or_default(&paths.config_file)?;

        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Ok(Self {
            kconfig,
            config,
            paths,
            current_menu_stack: vec![],
            list_state,
            show_help: false,
            dialog: DialogType::None,
            notification_timer: 0,
            input_buffer: String::new(),
            should_exit: false,
            config_modified: false,
        })
    }

    /// Run the TUI application
    pub fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main loop
        while !self.should_exit {
            terminal.draw(|f| self.ui(f))?;
            self.handle_events()?;
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

        Ok(())
    }

    /// Get current items to display (either menus or config items)
    fn current_items(&self) -> Vec<MenuItem> {
        if self.current_menu_stack.is_empty() {
            // At root level - show menus
            self.kconfig
                .menus
                .iter()
                .enumerate()
                .map(|(idx, menu)| MenuItem::Menu {
                    index: idx,
                    prompt: menu.prompt.clone(),
                })
                .collect()
        } else {
            // Inside a menu - show config items
            let menu_idx = *self.current_menu_stack.last().unwrap();
            if let Some(menu) = self.kconfig.menus.get(menu_idx) {
                menu.configs
                    .iter()
                    .map(|item| MenuItem::Config {
                        id: item.id.clone(),
                        prompt: item.prompt.clone(),
                        item_type: item.item_type.clone(),
                        value: self.get_config_value(&item.id),
                        choices: item.choices.clone(),
                    })
                    .collect()
            } else {
                vec![]
            }
        }
    }

    /// Get current menu title
    fn current_title(&self) -> String {
        if self.current_menu_stack.is_empty() {
            "XenOS Configuration".to_string()
        } else {
            let menu_idx = *self.current_menu_stack.last().unwrap();
            self.kconfig
                .menus
                .get(menu_idx)
                .map(|m| m.prompt.clone())
                .unwrap_or_else(|| "Menu".to_string())
        }
    }

    /// Get help text for current selection
    fn current_help(&self) -> String {
        let items = self.current_items();
        if let Some(selected) = self.list_state.selected() {
            if let Some(item) = items.get(selected) {
                match item {
                    MenuItem::Menu { index, prompt } => {
                        format!("Enter '{}' submenu", prompt)
                    }
                    MenuItem::Config { id, .. } => {
                        if let Some(menu_idx) = self.current_menu_stack.last() {
                            if let Some(menu) = self.kconfig.menus.get(*menu_idx) {
                                if let Some(cfg) = menu.configs.iter().find(|c| &c.id == id) {
                                    return cfg.help.clone().unwrap_or_default();
                                }
                            }
                        }
                        String::new()
                    }
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    }

    /// Get config value as string
    fn get_config_value(&self, id: &str) -> String {
        // Use reflection-like approach via Config fields
        match id {
            "KERNEL_ALLOC" => format!("{}", self.config.KERNEL_ALLOC),
            "KERNEL_AML" => format!("{}", self.config.KERNEL_AML),
            "DEBUG_KD_FORCE" => format!("{}", self.config.DEBUG_KD_FORCE),
            "TRACE_ENABLE" => format!("{}", self.config.TRACE_ENABLE),
            "TRACE_CALLS" => format!("{}", self.config.TRACE_CALLS),
            "TRACE_SCHED" => format!("{}", self.config.TRACE_SCHED),
            "TRACE_PS" => format!("{}", self.config.TRACE_PS),
            "TRACE_MM" => format!("{}", self.config.TRACE_MM),
            "TRACE_OB" => format!("{}", self.config.TRACE_OB),
            "TRACE_IO" => format!("{}", self.config.TRACE_IO),
            "TRACE_STORAGE" => format!("{}", self.config.TRACE_STORAGE),
            "TEST_KERNEL" => format!("{}", self.config.TEST_KERNEL),
            "TEST_PS" => format!("{}", self.config.TEST_PS),
            "QEMU_MACHINE" => self.config.QEMU_MACHINE.clone(),
            "QEMU_CPU" => self.config.QEMU_CPU.clone(),
            "QEMU_ACCEL" => self.config.QEMU_ACCEL.clone(),
            "QEMU_MEMORY" => self.config.QEMU_MEMORY.clone(),
            "QEMU_SMP" => format!("{}", self.config.QEMU_SMP),
            "QEMU_DISPLAY" => self.config.QEMU_DISPLAY.clone(),
            "QEMU_VGA" => self.config.QEMU_VGA.clone(),
            "QEMU_MONITOR" => self.config.QEMU_MONITOR.clone(),
            "QEMU_SERIAL" => self.config.QEMU_SERIAL.clone(),
            "QEMU_SERIAL_FILE" => self.config.QEMU_SERIAL_FILE.clone(),
            "BUILD_MODE" => self.config.BUILD_MODE.clone(),
            "BUILD_DRIVERS" => format!("{}", self.config.BUILD_DRIVERS),
            "BUILD_ISO" => format!("{}", self.config.BUILD_ISO),
            "BUILD_LIMINE" => format!("{}", self.config.BUILD_LIMINE),
            _ => "?".to_string(),
        }
    }

    /// Set config value
    fn set_config_value(&mut self, id: &str, value: &str) {
        match id {
            "KERNEL_ALLOC" => self.config.KERNEL_ALLOC = value == "true",
            "KERNEL_AML" => self.config.KERNEL_AML = value == "true",
            "DEBUG_KD_FORCE" => self.config.DEBUG_KD_FORCE = value == "true",
            "TRACE_ENABLE" => self.config.TRACE_ENABLE = value == "true",
            "TRACE_CALLS" => self.config.TRACE_CALLS = value == "true",
            "TRACE_SCHED" => self.config.TRACE_SCHED = value == "true",
            "TRACE_PS" => self.config.TRACE_PS = value == "true",
            "TRACE_MM" => self.config.TRACE_MM = value == "true",
            "TRACE_OB" => self.config.TRACE_OB = value == "true",
            "TRACE_IO" => self.config.TRACE_IO = value == "true",
            "TRACE_STORAGE" => self.config.TRACE_STORAGE = value == "true",
            "TEST_KERNEL" => self.config.TEST_KERNEL = value == "true",
            "TEST_PS" => self.config.TEST_PS = value == "true",
            "QEMU_MACHINE" => self.config.QEMU_MACHINE = value.to_string(),
            "QEMU_CPU" => self.config.QEMU_CPU = value.to_string(),
            "QEMU_ACCEL" => self.config.QEMU_ACCEL = value.to_string(),
            "QEMU_MEMORY" => self.config.QEMU_MEMORY = value.to_string(),
            "QEMU_SMP" => self.config.QEMU_SMP = value.parse().unwrap_or(1),
            "QEMU_DISPLAY" => self.config.QEMU_DISPLAY = value.to_string(),
            "QEMU_VGA" => self.config.QEMU_VGA = value.to_string(),
            "QEMU_MONITOR" => self.config.QEMU_MONITOR = value.to_string(),
            "QEMU_SERIAL" => self.config.QEMU_SERIAL = value.to_string(),
            "QEMU_SERIAL_FILE" => self.config.QEMU_SERIAL_FILE = value.to_string(),
            "BUILD_MODE" => self.config.BUILD_MODE = value.to_string(),
            "BUILD_DRIVERS" => self.config.BUILD_DRIVERS = value == "true",
            "BUILD_ISO" => self.config.BUILD_ISO = value == "true",
            "BUILD_LIMINE" => self.config.BUILD_LIMINE = value == "true",
            _ => {}
        }
        self.config_modified = true;
    }

    /// Toggle boolean value or open input dialog for string/int
    fn toggle_current(&mut self) {
        let items = self.current_items();
        if let Some(selected) = self.list_state.selected() {
            if let Some(MenuItem::Config {
                id,
                prompt,
                item_type,
                value,
                choices,
                ..
            }) = items.get(selected)
            {
                match item_type.as_str() {
                    "bool" => {
                        let new_val = if value == "true" { "false" } else { "true" };
                        self.set_config_value(id, new_val);
                    }
                    "choice" => {
                        if let Some(choices) = choices {
                            if let Some(idx) = choices.iter().position(|c| c == value) {
                                let next_idx = (idx + 1) % choices.len();
                                self.set_config_value(id, &choices[next_idx]);
                            } else if !choices.is_empty() {
                                self.set_config_value(id, &choices[0]);
                            }
                        }
                    }
                    "int" => {
                        // Open input dialog for int
                        self.input_buffer = value.clone();
                        self.dialog = DialogType::StringInput {
                            id: id.clone(),
                            prompt: prompt.clone(),
                        };
                    }
                    "string" => {
                        // Open input dialog for string
                        self.input_buffer = value.clone();
                        self.dialog = DialogType::StringInput {
                            id: id.clone(),
                            prompt: prompt.clone(),
                        };
                    }
                    _ => {}
                }
            }
        }
    }

    /// Decrement integer or cycle choices backward
    fn decrement_current(&mut self) {
        let items = self.current_items();
        if let Some(selected) = self.list_state.selected() {
            if let Some(MenuItem::Config {
                id,
                item_type,
                value,
                choices,
                ..
            }) = items.get(selected)
            {
                match item_type.as_str() {
                    "choice" => {
                        if let Some(choices) = choices {
                            if let Some(idx) = choices.iter().position(|c| c == value) {
                                let next_idx = if idx == 0 { choices.len() - 1 } else { idx - 1 };
                                self.set_config_value(id, &choices[next_idx]);
                            }
                        }
                    }
                    "int" => {
                        if let Ok(n) = value.parse::<i32>() {
                            if n > 1 {
                                self.set_config_value(id, &(n - 1).to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Handle keyboard events
    fn handle_events(&mut self) -> Result<()> {
        // Handle notification timer
        if self.notification_timer > 0 {
            self.notification_timer -= 1;
            if self.notification_timer == 0 {
                self.dialog = DialogType::None;
            }
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    return Ok(());
                }

                // Handle dialogs first
                match self.dialog {
                    DialogType::SaveConfirm => {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                                self.config.save(&self.paths.config_file)?;
                                self.config_modified = false;
                                self.dialog = DialogType::SavedNotification;
                                self.notification_timer = 15; // ~1.5 seconds
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                self.dialog = DialogType::None;
                            }
                            _ => {}
                        }
                        return Ok(());
                    }
                    DialogType::ExitConfirm => {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                                self.config.save(&self.paths.config_file)?;
                                self.should_exit = true;
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                // Exit without saving
                                self.should_exit = true;
                            }
                            KeyCode::Esc => {
                                // Cancel exit
                                self.dialog = DialogType::None;
                            }
                            _ => {}
                        }
                        return Ok(());
                    }
                    DialogType::SavedNotification => {
                        // Any key dismisses notification
                        self.dialog = DialogType::None;
                        self.notification_timer = 0;
                        return Ok(());
                    }
                    DialogType::StringInput { ref id, .. } => {
                        match key.code {
                            KeyCode::Enter => {
                                // Save the value
                                let id = id.clone();
                                let value = self.input_buffer.clone();
                                self.set_config_value(&id, &value);
                                self.dialog = DialogType::None;
                                self.input_buffer.clear();
                            }
                            KeyCode::Esc => {
                                // Cancel input
                                self.dialog = DialogType::None;
                                self.input_buffer.clear();
                            }
                            KeyCode::Backspace => {
                                self.input_buffer.pop();
                            }
                            KeyCode::Char(c) => {
                                self.input_buffer.push(c);
                            }
                            _ => {}
                        }
                        return Ok(());
                    }
                    DialogType::None => {}
                }

                // If help overlay is shown, any key closes it
                if self.show_help {
                    self.show_help = false;
                    return Ok(());
                }

                match key.code {
                    KeyCode::Char('q') => {
                        // q always quits from any level
                        if self.config_modified {
                            self.dialog = DialogType::ExitConfirm;
                        } else {
                            self.should_exit = true;
                        }
                    }
                    KeyCode::Esc => {
                        if self.current_menu_stack.is_empty() {
                            if self.config_modified {
                                self.dialog = DialogType::ExitConfirm;
                            } else {
                                self.should_exit = true;
                            }
                        } else {
                            // Go back to parent menu
                            self.current_menu_stack.pop();
                            self.list_state.select(Some(0));
                        }
                    }
                    KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Ctrl+S saves immediately without dialog
                        self.config.save(&self.paths.config_file)?;
                        self.config_modified = false;
                        self.dialog = DialogType::SavedNotification;
                        self.notification_timer = 15;
                    }
                    KeyCode::Char('s') | KeyCode::Char('S') => {
                        // Show save confirmation dialog
                        self.dialog = DialogType::SaveConfirm;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        let items = self.current_items();
                        if !items.is_empty() {
                            let selected = self.list_state.selected().unwrap_or(0);
                            let next = if selected == 0 {
                                items.len() - 1
                            } else {
                                selected - 1
                            };
                            self.list_state.select(Some(next));
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let items = self.current_items();
                        if !items.is_empty() {
                            let selected = self.list_state.selected().unwrap_or(0);
                            let next = (selected + 1) % items.len();
                            self.list_state.select(Some(next));
                        }
                    }
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        let items = self.current_items();
                        if let Some(selected) = self.list_state.selected() {
                            if let Some(item) = items.get(selected) {
                                match item {
                                    MenuItem::Menu { index, .. } => {
                                        self.current_menu_stack.push(*index);
                                        self.list_state.select(Some(0));
                                    }
                                    MenuItem::Config { .. } => {
                                        self.toggle_current();
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        let items = self.current_items();
                        if let Some(selected) = self.list_state.selected() {
                            if let Some(MenuItem::Config { item_type, .. }) = items.get(selected) {
                                if item_type == "choice" || item_type == "int" {
                                    self.decrement_current();
                                    return Ok(());
                                }
                            }
                        }
                        // Go back if not on a choice/int
                        if !self.current_menu_stack.is_empty() {
                            self.current_menu_stack.pop();
                            self.list_state.select(Some(0));
                        }
                    }
                    KeyCode::Char(' ') => {
                        // Space toggles current item
                        self.toggle_current();
                    }
                    KeyCode::Char('y') | KeyCode::Char('n') => {
                        let items = self.current_items();
                        if let Some(selected) = self.list_state.selected() {
                            if let Some(MenuItem::Config {
                                id, item_type, ..
                            }) = items.get(selected)
                            {
                                if item_type == "bool" {
                                    let new_val = if key.code == KeyCode::Char('n') {
                                        "false"
                                    } else {
                                        "true"
                                    };
                                    self.set_config_value(id, new_val);
                                }
                            }
                        }
                    }
                    KeyCode::Char('+') => {
                        self.toggle_current();
                    }
                    KeyCode::Char('-') => {
                        self.decrement_current();
                    }
                    KeyCode::Char('?') => {
                        self.show_help = !self.show_help;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Render the UI
    fn ui(&self, frame: &mut Frame) {
        let area = frame.area();

        // Split into main area and footer
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),
                Constraint::Length(3), // Help text
                Constraint::Length(1), // Status bar
            ])
            .split(area);

        // Main content area: split into list and help panel
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[0]);

        // Render menu/config list
        self.render_list(frame, main_chunks[0]);

        // Render help panel
        self.render_help_panel(frame, main_chunks[1]);

        // Render footer help
        self.render_footer_help(frame, chunks[1]);

        // Render status bar
        self.render_status_bar(frame, chunks[2]);

        // Show help overlay if enabled
        if self.show_help {
            self.render_help_overlay(frame, area);
        }

        // Show dialogs
        match &self.dialog {
            DialogType::SaveConfirm => self.render_save_dialog(frame, area),
            DialogType::SavedNotification => self.render_saved_notification(frame, area),
            DialogType::ExitConfirm => self.render_exit_dialog(frame, area),
            DialogType::StringInput { prompt, .. } => {
                self.render_string_input_dialog(frame, area, prompt)
            }
            DialogType::None => {}
        }
    }

    fn render_list(&self, frame: &mut Frame, area: Rect) {
        let items = self.current_items();
        let title = self.current_title();

        let list_items: Vec<ListItem> = items
            .iter()
            .map(|item| {
                let (marker, text) = match item {
                    MenuItem::Menu { prompt, .. } => {
                        ("  > ".to_string(), prompt.clone())
                    }
                    MenuItem::Config {
                        prompt,
                        item_type,
                        value,
                        choices,
                        ..
                    } => {
                        let marker = match item_type.as_str() {
                            "bool" => {
                                if value == "true" {
                                    "[*] ".to_string()
                                } else {
                                    "[ ] ".to_string()
                                }
                            }
                            "choice" => {
                                format!("<{}> ", value)
                            }
                            "int" => {
                                format!("({}) ", value)
                            }
                            "string" => {
                                format!("[{}] ", value)
                            }
                            _ => "    ".to_string(),
                        };
                        (marker, prompt.clone())
                    }
                };
                ListItem::new(format!("{}{}", marker, text))
            })
            .collect();

        let block = Block::default()
            .title(format!(" {} ", title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let list = List::new(list_items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.list_state.clone());
    }

    fn render_help_panel(&self, frame: &mut Frame, area: Rect) {
        let help_text = self.current_help();

        let block = Block::default()
            .title(" Help ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let paragraph = Paragraph::new(help_text)
            .block(block)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    fn render_footer_help(&self, frame: &mut Frame, area: Rect) {
        let help = if self.current_menu_stack.is_empty() {
            "Enter: Select | S: Save | q: Quit | ?: Help"
        } else {
            "Enter/Space: Toggle | Esc/h: Back | +/-: Change | S: Save | q: Quit | ?: Help"
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        let paragraph = Paragraph::new(help)
            .block(block)
            .style(Style::default().fg(Color::Gray));

        frame.render_widget(paragraph, area);
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let modified = if self.config_modified { " [MODIFIED]" } else { "" };
        let status = format!(
            " XenOS menuconfig{} | Config: {}",
            modified,
            self.paths.config_file.display()
        );

        let paragraph = Paragraph::new(status)
            .style(Style::default().fg(Color::White).bg(Color::DarkGray));

        frame.render_widget(paragraph, area);
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let help_text = r#"
Keyboard Shortcuts:

Navigation:
  Up/k        Move up
  Down/j      Move down
  Enter/l     Enter menu / Toggle option
  Left/h/Esc  Back to parent menu

Editing:
  Space/y     Enable option
  n           Disable option
  +/-         Cycle choice / Change integer
  Enter       Toggle boolean / Cycle choice

General:
  Ctrl+S      Save configuration
  q           Quit (saves if modified)
  ?           Toggle this help

Press any key to close..."#;

        // Create a centered overlay
        let popup_area = centered_rect(60, 70, area);

        let block = Block::default()
            .title(" Help ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let paragraph = Paragraph::new(help_text)
            .block(block)
            .style(Style::default().fg(Color::White));

        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }

    fn render_save_dialog(&self, frame: &mut Frame, area: Rect) {
        let popup_area = centered_rect(55, 35, area);

        let text = vec![
            Line::from(""),
            Line::from("Save configuration to:"),
            Line::from(""),
            Line::from(Span::styled(
                format!("{}", self.paths.config_file.display()),
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("[Y]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("es  "),
                Span::styled("[N]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("o"),
            ]),
        ];

        let block = Block::default()
            .title(" Save Configuration? ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        let paragraph = Paragraph::new(text)
            .block(block)
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Center);

        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }

    fn render_saved_notification(&self, frame: &mut Frame, area: Rect) {
        let popup_area = centered_rect(40, 20, area);

        let text = vec![
            Line::from(""),
            Line::from(Span::styled("✓ Configuration saved!", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(Span::styled("Press any key...", Style::default().fg(Color::DarkGray))),
        ];

        let block = Block::default()
            .title(" Saved ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        let paragraph = Paragraph::new(text)
            .block(block)
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Center);

        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }

    fn render_exit_dialog(&self, frame: &mut Frame, area: Rect) {
        let popup_area = centered_rect(50, 35, area);

        let text = vec![
            Line::from(""),
            Line::from("Configuration has been modified."),
            Line::from(""),
            Line::from("Save before exit?"),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("[Y]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("es  "),
                Span::styled("[N]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("o  "),
                Span::styled("[Esc]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(" Cancel"),
            ]),
        ];

        let block = Block::default()
            .title(" Exit ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let paragraph = Paragraph::new(text)
            .block(block)
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Center);

        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }

    fn render_string_input_dialog(&self, frame: &mut Frame, area: Rect, prompt: &str) {
        let popup_area = centered_rect(50, 30, area);

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(prompt, Style::default().fg(Color::Cyan))),
            Line::from(""),
            Line::from(vec![
                Span::raw("Value: "),
                Span::styled(
                    format!("{}_", self.input_buffer),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("[Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Save  "),
                Span::styled("[Esc]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" Cancel"),
            ]),
        ];

        let block = Block::default()
            .title(" Edit Value ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(text)
            .block(block)
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Center);

        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }
}

/// Menu item type for display
enum MenuItem {
    Menu {
        index: usize,
        prompt: String,
    },
    Config {
        id: String,
        prompt: String,
        item_type: String,
        value: String,
        choices: Option<Vec<String>>,
    },
}

/// Helper function to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

