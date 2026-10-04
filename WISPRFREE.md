# Wisprfree

Wisprfree est un fork de [Handy](https://github.com/cjpais/Handy) (licence MIT) qui le rapproche de Wispr Flow : la dictée apprend de toi et s'adapte à l'app où tu écris. Tout reste local, sauf si tu choisis un fournisseur IA dans le cloud pour le post-traitement.

## Ce qui change par rapport à Handy

### Apprendre une correction (Réglages > Avancé > Apprentissage)

1. Tu dictes, et un mot sort mal (« Tomas » au lieu de « Thomas »).
2. Tu le corriges dans ton texte, puis tu le sélectionnes.
3. Tu appuies sur **Ctrl+Maj+L** (macOS : **Ctrl+Option+L**).

Le mot rejoint tes mots personnalisés, et la paire `Tomas → Thomas` est ajoutée aux remplacements : la prochaine fois, la correction est automatique. Ça marche aussi pour les mots coupés (« chat GPT » → « ChatGPT ») et pour les majuscules (« github » → « GitHub »).

### Remplacements et snippets vocaux

Une liste `quand je dis… → écrire…` appliquée à chaque transcription. Les corrections apprises y apparaissent avec l'étiquette « appris ». Tu peux aussi t'en servir comme snippets : « mon adresse » → `12 rue X\n75000 Paris` (`\n` fait un retour à la ligne).

### Prompts par application (Réglages > Post-traitement)

Tu associes un prompt à une app : `slack|discord` → « Message court », `gmail|outlook` → « Email ». L'app est détectée au moment où tu commences à dicter, à partir de son nom ou du titre de sa fenêtre. Les autres apps utilisent le prompt sélectionné.

- Dans un prompt, `${app}` est remplacé par le nom de l'app.
- Tes mots personnalisés sont ajoutés automatiquement au prompt, pour que l'IA garde leur orthographe.

### Toujours post-traiter

Avec cette option, le raccourci principal applique aussi le post-traitement IA. Tu n'as plus besoin du raccourci séparé.

### Autres changements

- L'interrupteur du post-traitement IA a quitté les fonctions expérimentales : il est dans Réglages > Avancé > Transcription.
- L'app s'appelle Wisprfree (`com.wisprfree.app`). Elle s'installe à côté de Handy, avec ses propres réglages.
- Les mises à jour viennent de ton fork, jamais de Handy. Chaque build de `main` est publié comme version signée, et l'app propose « Mise à jour disponible » en bas de la fenêtre. Ça demande le secret GitHub `TAURI_SIGNING_PRIVATE_KEY` (la clé privée qui correspond à `pubkey` dans `src-tauri/tauri.conf.json`).

## Mettre à jour

Quand une nouvelle version est publiée, « Mise à jour disponible » apparaît en bas de la fenêtre des réglages. Un clic la télécharge, vérifie sa signature, l'installe et relance l'app. Tu peux aussi vérifier à la main depuis l'icône de la barre des menus, avec « Rechercher des mises à jour ». Tes réglages et tes modèles sont conservés.

Les versions sont publiées sur la page [Releases](https://github.com/dev-thomass/Handy/releases) du fork. Le fichier `Wisprfree-aarch64.dmg` est pour les Mac Apple Silicon (M1 et suivants), `Wisprfree-x86_64.dmg` pour les Mac Intel.

## Limites connues

- Détection de l'app active : Windows et macOS (nom de l'app) ; sous Linux, X11 (`xprop`) et Hyprland. Les autres compositeurs Wayland ne sont pas détectés, et le prompt sélectionné s'applique alors.
- « Apprendre une correction » simule Ctrl+C / Cmd+C pour lire ta sélection. Sous Wayland, il faut `wtype`, `dotool` ou `ydotool` (comme pour le collage).
- Les icônes et le logo sont encore ceux de Handy, et la marque Handy n'est pas sous licence libre. Il faut les remplacer avant toute diffusion publique.

## Compiler

Les instructions sont les mêmes que pour Handy, voir [BUILD.md](BUILD.md). En bref : `bun install`, puis `bun run tauri dev`. Sous Windows, `bun run tauri build` échoue à l'étape de signature (elle utilise le certificat de Handy). Il faut alors utiliser `bun run tauri build --no-bundle`, ou retirer `signCommand` de `src-tauri/tauri.conf.json`.
