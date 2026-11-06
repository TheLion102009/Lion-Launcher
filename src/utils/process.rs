//! Externe Programme ohne Konsolenfenster starten.
//!
//! Der Launcher ist unter Windows eine GUI-App ohne eigene Konsole
//! (`windows_subsystem = "windows"`). Startet er ein Konsolenprogramm wie `java.exe`,
//! `powershell` oder `taskkill`, öffnet Windows dafür jedes Mal ein eigenes Terminal –
//! auch wenn stdout/stderr umgeleitet sind. `CREATE_NO_WINDOW` unterdrückt das.

/// Win32 `CREATE_NO_WINDOW`
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub trait HideConsoleWindow {
    /// Unter Windows ohne Konsolenfenster starten; auf anderen Systemen ohne Wirkung.
    fn hide_console_window(&mut self) -> &mut Self;
}

impl HideConsoleWindow for std::process::Command {
    fn hide_console_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(CREATE_NO_WINDOW);
        }
        self
    }
}

impl HideConsoleWindow for tokio::process::Command {
    fn hide_console_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        self.creation_flags(CREATE_NO_WINDOW);
        self
    }
}
