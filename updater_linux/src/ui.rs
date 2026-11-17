//! Interface en ligne de commande, à la place de `gui.rs` (TaskDialog) : tout passe par stdout, stderr et
//! stdin. Sans terminal (lancé par le jeu), rien n'est demandé : `main.rs` n'appelle `choose` et
//! `confirm` que si `interactive()`.

use std::io::{BufRead, IsTerminal, Write};

#[derive(Clone, Copy)]
pub enum Icon {
    Info,
    Warning,
    Error,
}

/// Vrai si stdin et stdout sont un terminal : on peut poser des questions.
pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Message : l'erreur et l'avertissement sur stderr, le reste sur stdout.
pub fn message(icon: Icon, title: &str, body: &str) {
    let text = if body.is_empty() { title.to_string() } else { format!("{title}\n{body}") };
    match icon {
        Icon::Info => println!("{text}"),
        Icon::Warning => eprintln!("[!] {text}"),
        Icon::Error => eprintln!("[x] {text}"),
    }
}

/// Exécute `work` en annonçant `label` (pas de barre de progression : une ligne suffit).
pub fn progress<T>(label: &str, work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    println!("{label}…");
    work()
}

/// Menu numéroté : le numéro choisi (à partir de 0), ou `None` pour quitter (ligne vide, fin de
/// fichier ou numéro hors liste).
pub fn choose(prompt: &str, options: &[&str]) -> Option<usize> {
    for (i, option) in options.iter().enumerate() {
        println!("  {}. {option}", i + 1);
    }
    print!("{prompt} ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok()?;
    let n: usize = line.trim().parse().ok()?;
    (1..=options.len()).contains(&n).then(|| n - 1)
}
