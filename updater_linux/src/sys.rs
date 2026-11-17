//! Appels au système Linux, à la place de `win.rs` : racines de Steam, liste des processus (`/proc`),
//! ouverture d'une adresse, notification du bureau.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Dossier d'une variable XDG : la variable si elle est définie et non vide, sinon `$HOME/<défaut>`.
pub fn xdg_dir(var: &str, default: &str, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    match env(var).filter(|v| !v.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => Some(PathBuf::from(env("HOME").filter(|v| !v.is_empty())?).join(default)),
    }
}

pub fn env(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// Racines possibles de Steam, par ordre de préférence. Pas de registre sous Linux : on essaie les
/// emplacements connus. (Flatpak et Snap ne sont pas pris en compte.)
pub fn steam_roots() -> Vec<PathBuf> {
    steam_roots_with(&env)
}

pub fn steam_roots_with(env: &dyn Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(data) = xdg_dir("XDG_DATA_HOME", ".local/share", env) {
        roots.push(data.join("Steam"));
    }
    if let Some(home) = env("HOME").filter(|v| !v.is_empty()) {
        let home = PathBuf::from(home);
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".steam/steam"));
    }
    roots
}

/// Noms (`comm`) des processus de `proc`, sauf le processus `own` (l'installeur lui-même). `comm` est
/// tronqué à 15 caractères : seul le début d'un nom est fiable.
pub fn process_names_in(proc: &Path, own: u32) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(proc) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()).is_some_and(|pid| pid != own))
        .filter_map(|e| std::fs::read_to_string(e.path().join("comm")).ok())
        .map(|name| name.trim_end().to_string())
        .collect()
}

pub fn process_names() -> Vec<String> {
    process_names_in(Path::new("/proc"), std::process::id())
}

/// Ouvre une adresse (`steam://…`, page web) par `xdg-open`, sans attendre.
pub fn open(target: &str) -> bool {
    Command::new("xdg-open")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Notification du bureau, si `notify-send` existe ; sans effet sinon.
pub fn notify(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .arg("--app-name=BetterGameExperience")
        .arg(title)
        .arg(body)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| OsString::from(*v))
    }

    #[test]
    fn xdg() {
        let env = fake(&[("HOME", "/home/u"), ("XDG_DATA_HOME", "/data")]);
        assert_eq!(xdg_dir("XDG_DATA_HOME", ".local/share", &env), Some(PathBuf::from("/data")));
        // Vide : comme si elle n'était pas définie.
        let env = fake(&[("HOME", "/home/u"), ("XDG_DATA_HOME", "")]);
        assert_eq!(xdg_dir("XDG_DATA_HOME", ".local/share", &env), Some(PathBuf::from("/home/u/.local/share")));
        assert_eq!(xdg_dir("XDG_DATA_HOME", ".local/share", &fake(&[])), None);
    }

    #[test]
    fn racines_de_steam() {
        let env = fake(&[("HOME", "/home/u"), ("XDG_DATA_HOME", "/data")]);
        assert_eq!(
            steam_roots_with(&env),
            [PathBuf::from("/data/Steam"), "/home/u/.local/share/Steam".into(), "/home/u/.steam/steam".into()]
        );
    }

    #[test]
    fn processus_sauf_soi_meme() {
        let proc = std::env::temp_dir().join(format!("bgd_proc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&proc);
        for (pid, name) in [("10", "CivilizationVI_\n"), ("11", "bash\n"), ("12", "CivilizationVI\n")] {
            std::fs::create_dir_all(proc.join(pid)).unwrap();
            std::fs::write(proc.join(pid).join("comm"), name).unwrap();
        }
        std::fs::create_dir_all(proc.join("self")).unwrap();
        let mut names = process_names_in(&proc, 12);
        names.sort();
        assert_eq!(names, ["CivilizationVI_", "bash"]);
        std::fs::remove_dir_all(&proc).unwrap();
    }
}
