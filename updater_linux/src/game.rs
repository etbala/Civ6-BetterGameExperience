//! Dossier de Civilization VI : bibliothèques Steam (`libraryfolders.vdf`), ou dossier donné.

use std::fs;
use std::path::{Path, PathBuf};

use crate::sys;
use crate::vdf;

/// Civilization VI sur Steam.
const APP_ID: &str = "289070";
const DEFAULT_DIR: &str = "Sid Meier's Civilization VI";

/// Un dossier est celui du jeu s'il contient le GameCore de Gathering Storm.
pub fn is_game_dir(dir: &Path) -> bool {
    dir.join("DLC/Expansion2/Binaries/Win64/GameCore_XP2_FinalRelease.dll").is_file()
}

/// Résultat d'une recherche qui échoue : ce qui a été regardé, pour le dire au joueur.
pub struct NotFound {
    /// Bibliothèques lues, sans le jeu.
    pub checked: Vec<PathBuf>,
    /// Bibliothèques listées par Steam mais introuvables (disque non monté…).
    pub unreachable: Vec<PathBuf>,
}

impl NotFound {
    /// Une ligne par bibliothèque, pour le message d'erreur.
    pub fn list(&self) -> String {
        let mut lines: Vec<String> = self.checked.iter().map(|p| format!("  {}", p.display())).collect();
        lines.extend(self.unreachable.iter().map(|p| format!("  {} (?)", p.display())));
        if lines.is_empty() {
            lines.push("  -".into());
        }
        lines.join("\n")
    }
}

/// Le jeu, dans l'une des bibliothèques Steam (`steamapps/libraryfolders.vdf`).
pub fn find() -> Result<PathBuf, NotFound> {
    find_in(&sys::steam_roots())
}

pub fn find_in(roots: &[PathBuf]) -> Result<PathBuf, NotFound> {
    let mut libraries: Vec<PathBuf> = Vec::new();
    for steam in roots.iter().filter(|r| r.is_dir()) {
        libraries.push(steam.clone());
        if let Ok(text) = fs::read_to_string(steam.join("steamapps/libraryfolders.vdf")) {
            libraries.extend(library_paths(&text));
        }
    }
    let mut checked: Vec<PathBuf> = Vec::new();
    let mut unreachable = Vec::new();
    for lib in libraries {
        // Chemin canonique : un lien symbolique, ou deux racines de Steam, ne comptent qu'une fois.
        let Ok(lib) = fs::canonicalize(&lib) else {
            if !unreachable.contains(&lib) {
                unreachable.push(lib);
            }
            continue;
        };
        if checked.contains(&lib) {
            continue;
        }
        let dir = lib.join("steamapps/common").join(install_dir(&lib));
        if is_game_dir(&dir) {
            return Ok(dir);
        }
        checked.push(lib);
    }
    Err(NotFound { checked, unreachable })
}

/// Les `"path"` de chaque bibliothèque de `libraryfolders.vdf`.
fn library_paths(text: &str) -> Vec<PathBuf> {
    let Ok(root) = vdf::parse(text) else { return Vec::new() };
    let Some(folders) = vdf::get_block(&root, "libraryfolders") else { return Vec::new() };
    folders
        .iter()
        .filter_map(|(_, v)| match v {
            vdf::Value::Block(b) => vdf::get_str(b, "path").map(PathBuf::from),
            vdf::Value::Str(_) => None,
        })
        .collect()
}

/// Nom du dossier du jeu dans `steamapps/common`, d'après `appmanifest_289070.acf`.
fn install_dir(library: &Path) -> String {
    let manifest = fs::read_to_string(library.join(format!("steamapps/appmanifest_{APP_ID}.acf"))).ok();
    manifest
        .and_then(|text| vdf::parse(&text).ok())
        .and_then(|root| vdf::get_block(&root, "AppState").and_then(|s| vdf::get_str(s, "installdir")).map(str::to_string))
        // Un nom qui sortirait de `common` n'est pas un nom de dossier.
        .filter(|d| !d.is_empty() && !d.contains('/') && d != "..")
        .unwrap_or_else(|| DEFAULT_DIR.to_string())
}

/// Le jeu, parmi les dossiers parents de `exe` (dossier donné par `--after-game`).
pub fn from_command(exe: &Path) -> Option<PathBuf> {
    exe.ancestors().find(|dir| is_game_dir(dir)).map(Path::to_path_buf)
}

/// Civilization VI est lancé : `CivilizationVI.exe`, `CivilizationVI_DX12.exe`… Seul le début de `comm`
/// est fiable (15 caractères au plus) ; la ligne de commande n'est pas regardée, l'installeur lancé
/// par le jeu y porte « Civilization VI ».
pub fn running() -> bool {
    sys::process_names().iter().any(|n| is_game_process(n))
}

fn is_game_process(name: &str) -> bool {
    name.to_lowercase().starts_with("civilizationvi")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bgd_game_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn install(library: &Path, dir: &str) {
        let marker = library.join("steamapps/common").join(dir).join("DLC/Expansion2/Binaries/Win64/GameCore_XP2_FinalRelease.dll");
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(marker, b"").unwrap();
    }

    fn libraryfolders(steam: &Path, paths: &[&Path]) {
        let mut text = String::from("\"libraryfolders\"\n{\n");
        for (i, p) in paths.iter().enumerate() {
            text += &format!("\t\"{i}\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n", p.display());
        }
        text += "}\n";
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        fs::write(steam.join("steamapps/libraryfolders.vdf"), text).unwrap();
    }

    #[test]
    fn jeu_dans_une_deuxieme_bibliotheque() {
        let dir = tmp("deuxieme");
        let (steam, second) = (dir.join("Steam"), dir.join("Jeux/SteamLibrary"));
        fs::create_dir_all(&second).unwrap();
        libraryfolders(&steam, &[&steam, &second]);
        install(&second, DEFAULT_DIR);
        let found = find_in(&[steam.clone()]).ok().unwrap();
        assert_eq!(found, fs::canonicalize(&second).unwrap().join("steamapps/common").join(DEFAULT_DIR));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn installdir_du_manifeste() {
        let dir = tmp("installdir");
        let steam = dir.join("Steam");
        libraryfolders(&steam, &[&steam]);
        install(&steam, "Civ6");
        fs::write(steam.join("steamapps/appmanifest_289070.acf"), "\"AppState\"\n{\n\t\"installdir\"\t\t\"Civ6\"\n}\n").unwrap();
        assert!(find_in(&[steam.clone()]).is_ok());
        // Un nom qui sort de `common` est ignoré : retour au nom par défaut.
        fs::write(steam.join("steamapps/appmanifest_289070.acf"), "\"AppState\"\n{\n\t\"installdir\"\t\t\"../x\"\n}\n").unwrap();
        assert_eq!(install_dir(&steam), DEFAULT_DIR);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn introuvable_dit_ce_qui_a_ete_regarde() {
        let dir = tmp("introuvable");
        let (steam, missing) = (dir.join("Steam"), dir.join("pas_monte"));
        libraryfolders(&steam, &[&steam, &missing]);
        let Err(not_found) = find_in(&[steam.clone()]) else { panic!("trouvé") };
        assert_eq!(not_found.checked, [fs::canonicalize(&steam).unwrap()]);
        assert_eq!(not_found.unreachable, [missing]);
        assert!(not_found.list().contains("pas_monte"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bibliotheque_en_lien_symbolique_comptee_une_fois() {
        let dir = tmp("lien");
        let (steam, real) = (dir.join("Steam"), dir.join("reel"));
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, dir.join("lien")).unwrap();
        libraryfolders(&steam, &[&steam, &real, &dir.join("lien")]);
        let Err(not_found) = find_in(&[steam.clone(), steam.clone()]) else { panic!("trouvé") };
        assert_eq!(not_found.checked.len(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn processus_du_jeu() {
        assert!(is_game_process("CivilizationVI"));
        assert!(is_game_process("CivilizationVI_"));
        assert!(!is_game_process("BetterGameExperience"));
        assert!(!is_game_process("steam"));
    }
}
