//! BetterGameExperience (Linux) : installe, met à jour, répare ou retire les proxys de
//! BetterGameExperience dans Civilization VI sous Steam Play (Proton), à partir des releases GitHub.
//! Même structure que updater/src/main.rs (la logique de release et d'installation en est copiée),
//! mais en ligne de commande : pas de fenêtre, pas d'administrateur.
//!
//!   BetterGameExperience                              menu (dans un terminal)
//!   BetterGameExperience status                       état de l'installation
//!   BetterGameExperience install                      installe ou met à jour
//!   BetterGameExperience uninstall                    retire les proxys
//!   BetterGameExperience launch-option                dit quelle option de lancement régler dans Steam
//!   BetterGameExperience channel release|prerelease   choisit le canal
//!   BetterGameExperience lang <code>                  choisit la langue
//!   --game-dir <dossier>                              jeu hors des bibliothèques Steam
//!   --yes                                             pas de confirmation
//!   BetterGameExperience --after-game [<dossier>]     (lancé par le jeu, sans terminal) attend la
//!                                                     fermeture du jeu, puis met à jour ; sans
//!                                                     question, résultat dans update.log
//!   --relaunch                                        avec --after-game : relance ensuite le jeu
//!
//! L'option de lancement de Steam (`localconfig.vdf`) n'est jamais modifiée ici : `launch-option`
//! donne les étapes au joueur.

mod game;
mod http;
mod install;
mod lang;
#[path = "../../proxy/src/noms.rs"]
mod noms;
mod release;
mod sys;
mod textes;
mod ui;
mod vdf;

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use install::{Action, State};
use lang::{f, t};
use textes as tx;
use ui::Icon;

/// Civilization VI sur Steam.
const STEAM_RUN: &str = "steam://rungameid/289070";

/// À saisir dans les options de lancement de Civilization VI : sans `winmm=n,b`, Wine charge sa propre
/// `winmm` et ignore celle de BetterGameExperience, à côté de l'exe.
const LAUNCH_OPTION: &str = "WINEDLLOVERRIDES=\"winmm=n,b\" %command%";

/// Script lancé par le jeu (le proxy de démarrage ne peut lancer qu'un script : Wine exécute un
/// binaire statique comme un module Wine). Il rend à l'installeur l'environnement d'un processus
/// ordinaire, sans ce que Steam y met (`LD_PRELOAD`…), et le détache du jeu.
const LAUNCHER: &str = "#!/bin/sh
# Généré par BetterGameExperience : ne pas modifier à la main.
dir=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd) || exit 1
unset LD_PRELOAD
if [ -n \"${SYSTEM_LD_LIBRARY_PATH+x}\" ]; then
    LD_LIBRARY_PATH=$SYSTEM_LD_LIBRARY_PATH
    export LD_LIBRARY_PATH
else
    unset LD_LIBRARY_PATH
fi
if command -v setsid >/dev/null 2>&1; then
    exec setsid \"$dir/BetterGameExperience\" \"$@\" </dev/null >/dev/null 2>&1
fi
exec \"$dir/BetterGameExperience\" \"$@\" </dev/null >/dev/null 2>&1
";

fn main() {
    lang::init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let yes = take_flag(&mut args, "--yes");
    let relaunch = take_flag(&mut args, "--relaunch");
    let game_dir = match take_value(&mut args, "--game-dir") {
        Ok(dir) => dir.map(PathBuf::from),
        Err(()) => return usage(),
    };
    let code = match args.first().map(String::as_str) {
        Some("--after-game") => after_game(args.get(1).map(PathBuf::from), relaunch),
        Some("status") => run(status(game_dir)),
        Some("install") => run(install_command(game_dir)),
        Some("uninstall") => run(uninstall_command(game_dir, yes)),
        Some("launch-option") => {
            println!("{}", launch_option_steps());
            0
        }
        Some("channel") => match args.get(1).map(String::as_str) {
            Some(c @ ("release" | "prerelease")) => {
                release::set_prerelease(c == "prerelease");
                println!("{c}");
                0
            }
            _ => return usage(),
        },
        Some("lang") => match args.get(1).and_then(|c| lang::LANGUAGES.iter().position(|l| l.code == c)) {
            Some(index) => {
                lang::set(index);
                println!("{}", lang::LANGUAGES[index].label);
                0
            }
            None => {
                let codes: Vec<&str> = lang::LANGUAGES.iter().map(|l| l.code).collect();
                eprintln!("{}", codes.join(" "));
                std::process::exit(2);
            }
        },
        None if ui::interactive() => run(interactive(game_dir)),
        _ => return usage(),
    };
    std::process::exit(code);
}

/// Sans terminal ni commande connue : le dit sur stderr (code 2).
fn usage() {
    eprintln!("{}", t(tx::LINUX_RUN_FROM_A_TERMINAL));
    std::process::exit(2);
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let found = args.iter().position(|a| a == flag);
    if let Some(i) = found {
        args.remove(i);
    }
    found.is_some()
}

/// Valeur de `flag <valeur>`, retirée des arguments ; `Err` si la valeur manque.
fn take_value(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, ()> {
    let Some(i) = args.iter().position(|a| a == flag) else { return Ok(None) };
    if i + 1 >= args.len() {
        return Err(());
    }
    args.remove(i);
    Ok(Some(args.remove(i)))
}

fn run(result: Result<(), String>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => failed(&e),
    }
}

fn failed(e: &str) -> i32 {
    ui::message(Icon::Error, t(tx::THE_OPERATION_FAILED), e);
    1
}

fn find_game(dir: Option<PathBuf>) -> Result<PathBuf, String> {
    match dir {
        Some(dir) => game::from_command(&dir).ok_or_else(|| f(tx::X_IS_NOT_THE_CIVILIZATION_VI, &[&dir.display()])),
        None => game::find().map_err(|not_found| f(tx::LINUX_GAME_NOT_FOUND_X, &[&not_found.list()])),
    }
}

/// Étapes à suivre par le joueur dans Steam : l'option de lancement n'est jamais écrite par l'installeur.
fn launch_option_steps() -> String {
    f(tx::LINUX_LAUNCH_OPTION_STEPS_X, &[&LAUNCH_OPTION])
}

fn status(game: Option<PathBuf>) -> Result<(), String> {
    let game = find_game(game)?;
    let manifest = release::fetch_manifest(&release::work_dir());
    show(&describe(&game, &manifest));
    Ok(())
}

fn install_command(game: Option<PathBuf>) -> Result<(), String> {
    copy_self();
    let game = find_game(game)?;
    let work = release::work_dir();
    let m = release::fetch_manifest(&work)?;
    if install::status(&game, Some(&m)).iter().all(|(_, s)| *s == State::UpToDate) {
        show(&describe(&game, &Ok(m)));
        return Ok(());
    }
    ensure_closed()?;
    install_release(&m, &game, &work)?;
    installed(&m.version);
    Ok(())
}

fn uninstall_command(game: Option<PathBuf>, yes: bool) -> Result<(), String> {
    let game = find_game(game)?;
    if !yes && ui::interactive() {
        let options = [t(tx::UNINSTALL_RESTORES_THE_GAME_S_ORIGINAL), t(tx::CLOSE)];
        if ui::choose(t(tx::LINUX_CHOOSE_A_NUMBER), &options) != Some(0) {
            return Ok(());
        }
    }
    ensure_closed()?;
    uninstall(&game)
}

fn ensure_closed() -> Result<(), String> {
    if game::running() {
        return Err(t(tx::CLOSE_CIVILIZATION_VI_THEN_RUN_THE).into());
    }
    Ok(())
}

fn uninstall(game: &Path) -> Result<(), String> {
    let work = release::work_dir();
    ui::progress(t(tx::UNINSTALLING_BETTERGAMEDLLS), || install::run(Action::Uninstall, game, &work))?;
    release::remember_installed(None);
    ui::message(Icon::Info, t(tx::BETTERGAMEDLLS_IS_UNINSTALLED), t(tx::THE_GAME_HAS_ITS_ORIGINAL_FILES));
    Ok(())
}

fn interactive(game: Option<PathBuf>) -> Result<(), String> {
    // Dès l'ouverture, pas seulement après une installation : un joueur déjà à jour, qui lance un
    // installeur plus récent, met aussi à jour celui que la notification du jeu lance.
    copy_self();
    let game = find_game(game)?;
    let work = release::work_dir();
    let mut manifest = release::fetch_manifest(&work);
    // Refaite à chaque changement de canal.
    loop {
        let view = describe(&game, &manifest);
        show(&view);
        enum Entry {
            Install,
            Uninstall,
            Toggle,
            LaunchOption,
        }
        let mut entries = Vec::new();
        if !view.install_text.is_empty() {
            entries.push((Entry::Install, view.install_text.clone()));
        }
        if view.ours > 0 || view.old {
            entries.push((Entry::Uninstall, t(tx::UNINSTALL_RESTORES_THE_GAME_S_ORIGINAL).to_string()));
        }
        let mark = if view.prerelease { "x" } else { " " };
        entries.push((Entry::Toggle, format!("[{mark}] {}", t(tx::PRE_RELEASE_INSTALL_TEST_VERSIONS))));
        entries.push((Entry::LaunchOption, t(tx::LINUX_SHOW_THE_LAUNCH_OPTION).to_string()));
        let labels: Vec<&str> = entries.iter().map(|(_, l)| l.as_str()).chain([t(tx::CLOSE)]).collect();
        let Some(choice) = ui::choose(t(tx::LINUX_CHOOSE_A_NUMBER), &labels) else { return Ok(()) };
        match entries.get(choice).map(|(e, _)| e) {
            Some(Entry::Install) => {
                ensure_closed()?;
                let m = manifest?;
                install_release(&m, &game, &work)?;
                installed(&m.version);
                return Ok(());
            }
            Some(Entry::Uninstall) => {
                ensure_closed()?;
                return uninstall(&game);
            }
            Some(Entry::Toggle) => {
                release::set_prerelease(!release::prerelease());
                let mut fetched = Err(String::new());
                let _ = ui::progress(t(tx::LOOKING_FOR_THE_LATEST_VERSION), || {
                    fetched = release::fetch_manifest(&work);
                    Ok(())
                });
                manifest = fetched;
            }
            Some(Entry::LaunchOption) => {
                println!("{}", launch_option_steps());
                return Ok(());
            }
            None => return Ok(()),
        }
    }
}

/// Ce que la fenêtre principale de Windows (`ask_action`) affiche : ici, du texte.
struct View {
    icon: Icon,
    instruction: String,
    content: String,
    details: Vec<String>,
    /// Texte de l'action d'installation ; vide s'il n'y a rien à installer.
    install_text: String,
    /// Proxys posés, et ancienne installation à remplacer.
    ours: usize,
    old: bool,
    prerelease: bool,
}

fn describe(game: &Path, manifest: &Result<release::Manifest, String>) -> View {
    let states = install::status(game, manifest.as_ref().ok());
    let ours = states.iter().filter(|(_, s)| matches!(s, State::Installed | State::UpToDate)).count();
    let old = states.iter().any(|(_, s)| *s == State::Old);
    let up_to_date = states.iter().all(|(_, s)| *s == State::UpToDate);
    let prerelease = release::prerelease();
    // Version posée plus récente que celle du canal : une pré-release, que le canal release
    // remplace par la dernière release (retour en arrière).
    let installed = release::installed_version();
    let downgrade = match (manifest, &installed) {
        (Ok(m), Some(v)) if ours > 0 && !up_to_date && release::is_newer(v, &m.version) => Some(v.clone()),
        _ => None,
    };

    let mut content = f(tx::GAME_X, &[&game.display()]);
    if prerelease {
        content += t(tx::CHANNEL_PRE_RELEASE_TEST_VERSIONS_AHEAD);
    }
    let (icon, instruction, install_text) = match manifest {
        Err(e) => {
            content += &f(tx::COULD_NOT_READ_THE_LATEST_VERSION, &[e]);
            (Icon::Warning, t(tx::LATEST_VERSION_UNKNOWN).to_string(), String::new())
        }
        Ok(m) if up_to_date => (Icon::Info, f(tx::BETTERGAMEDLLS_X_IS_INSTALLED_AND_UP, &[&m.version]), String::new()),
        Ok(m) if ours == 0 && !old => (
            Icon::Info,
            t(tx::BETTERGAMEDLLS_IS_NOT_INSTALLED).to_string(),
            f(tx::INSTALL_DOWNLOADS_AND_INSTALLS_VERSION_X, &[&m.version]),
        ),
        Ok(m) if old => (
            Icon::Info,
            f(tx::BETTERGAMEDLLS_VERSION_X_IS_AVAILABLE, &[&m.version]),
            f(tx::UPDATE_INSTALLS_VERSION_X_WHICH_NO, &[&m.version]),
        ),
        // Une partie seulement : un fichier a été retiré à la main.
        Ok(m) if ours < install::TARGETS.len() => (
            Icon::Warning,
            t(tx::BETTERGAMEDLLS_NEEDS_TO_BE_REPAIRED).to_string(),
            f(tx::REPAIR_REINSTALLS_VERSION_X, &[&m.version]),
        ),
        Ok(m) => match &downgrade {
            Some(v) => (
                Icon::Info,
                f(tx::BETTERGAMEDLLS_VERSION_X_IS_INSTALLED, &[v]),
                f(tx::GO_BACK_TO_VERSION_X_REPLACES, &[&m.version]),
            ),
            None => (
                Icon::Info,
                f(tx::BETTERGAMEDLLS_VERSION_X_IS_AVAILABLE, &[&m.version]),
                f(tx::UPDATE_INSTALLS_VERSION_X, &[&m.version]),
            ),
        },
    };
    let mut details: Vec<String> = states
        .iter()
        .map(|(name, state)| {
            let text = match state {
                State::Absent => t(tx::NOT_INSTALLED),
                State::Foreign => t(tx::FILE_FROM_ANOTHER_PROGRAM),
                State::Old => t(tx::OLD_PROXY_IN_PLACE_OF_THE),
                State::Installed => t(tx::PROXY_INSTALLED_OTHER_VERSION),
                State::UpToDate => t(tx::PROXY_UP_TO_DATE),
            };
            format!("{name} : {text}")
        })
        .collect();
    if let (Some(v), true) = (&installed, ours > 0) {
        details.push(f(tx::VERSION_INSTALLED_BY_THE_INSTALLER_X, &[v]));
    }
    View { icon, instruction, content, details, install_text, ours, old, prerelease }
}

fn show(view: &View) {
    println!();
    ui::message(view.icon, &view.instruction, &view.content);
    for line in &view.details {
        println!("  {line}");
    }
    println!();
}

fn install_release(m: &release::Manifest, game: &Path, work: &Path) -> Result<(), String> {
    ui::progress(&f(tx::INSTALLING_BETTERGAMEDLLS_X, &[&m.version]), || {
        release::fetch_files(m, work)?;
        install::run(Action::Install, game, work)
    })?;
    release::remember_installed(Some(&m.version));
    copy_self();
    Ok(())
}

/// Fin d'installation : dit que tout est en place, et ce qu'il reste à régler dans Steam.
fn installed(version: &str) {
    ui::message(Icon::Info, &f(tx::BETTERGAMEDLLS_X_IS_INSTALLED, &[&version]), &launch_option_steps());
}

/// Écrit `data` dans `path` par un fichier temporaire voisin, puis un renommage : un installeur ou un
/// script en cours d'exécution n'est jamais remplacé à moitié.
fn plant(path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("fichier");
    let temporary = path.with_file_name(format!(".{name}.new-{}", std::process::id()));
    let result = fs::write(&temporary, data)
        .and_then(|_| fs::set_permissions(&temporary, fs::Permissions::from_mode(mode)))
        .and_then(|_| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Copie l'installeur et son script de lancement dans le dossier de données, où le proxy de démarrage
/// les cherche (`bge-launch.sh`) : la copie suit le dernier installeur lancé.
fn copy_self() {
    let Some(dir) = release::data_dir() else { return };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stable = dir.join("BetterGameExperience");
    if let Ok(me) = std::env::current_exe() {
        if fs::canonicalize(&me).ok() != fs::canonicalize(&stable).ok() {
            if let Ok(data) = fs::read(&me) {
                let _ = plant(&stable, &data, 0o755);
            }
        }
    }
    let _ = plant(&dir.join("bge-launch.sh"), LAUNCHER.as_bytes(), 0o755);
}

/// Une ligne de `update.log` : l'installeur lancé par le jeu n'a pas de terminal.
fn log(line: &str) {
    let Some(dir) = release::data_dir() else { return };
    let _ = fs::create_dir_all(&dir);
    let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(dir.join("update.log")) else { return };
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let _ = writeln!(file, "{secs} {line}");
}

/// Lancé par la notification du jeu, qui se ferme : attend sa fin (une minute au plus), met à jour,
/// et en garde la trace dans `update.log` et dans une notification du bureau. Ne pose aucune question.
fn after_game(dir: Option<PathBuf>, relaunch: bool) -> i32 {
    for _ in 0..120 {
        if !game::running() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let result = (|| {
        if game::running() {
            return Err(t(tx::CIVILIZATION_VI_DID_NOT_CLOSE_CLOSE).to_string());
        }
        // Le dossier reçu n'est qu'une indication : revérifié, sinon le jeu est cherché.
        let game = match dir.and_then(|d| game::from_command(&d)) {
            Some(game) => game,
            None => find_game(None)?,
        };
        let work = release::work_dir();
        let m = release::fetch_manifest(&work)?;
        install_release(&m, &game, &work)?;
        Ok::<_, String>(m.version)
    })();
    match result {
        Ok(version) => {
            let message = f(tx::BETTERGAMEDLLS_X_IS_INSTALLED, &[&version]);
            log(&message);
            sys::notify("BetterGameExperience", &message);
            if relaunch {
                sys::open(STEAM_RUN);
            }
            0
        }
        Err(e) => {
            log(&format!("{}: {e}", t(tx::THE_OPERATION_FAILED)));
            sys::notify("BetterGameExperience", t(tx::THE_OPERATION_FAILED));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments() {
        let mut args: Vec<String> = ["--game-dir", "/g", "status", "--yes"].map(String::from).into();
        assert!(take_flag(&mut args, "--yes"));
        assert!(!take_flag(&mut args, "--relaunch"));
        assert_eq!(take_value(&mut args, "--game-dir"), Ok(Some("/g".to_string())));
        assert_eq!(args, ["status"]);
        let mut args: Vec<String> = ["--game-dir"].map(String::from).into();
        assert_eq!(take_value(&mut args, "--game-dir"), Err(()));
    }

    #[test]
    fn script_de_lancement() {
        // Les arguments sont transmis tels quels, sans eval.
        assert!(LAUNCHER.contains("\"$@\""));
        assert!(!LAUNCHER.contains("eval"));
        assert!(LAUNCHER.starts_with("#!/bin/sh\n"));
    }

    #[test]
    fn option_de_lancement() {
        assert_eq!(LAUNCH_OPTION, "WINEDLLOVERRIDES=\"winmm=n,b\" %command%");
        assert!(launch_option_steps().contains(LAUNCH_OPTION));
    }
}
