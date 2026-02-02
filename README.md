# Silex 🪨

[![Rust](https://img.shields.io/badge/built_with-Rust-dca282.svg)](https://www.rust-lang.org/)
[![SQLite](https://img.shields.io/badge/powered_by-SQLite-003B57.svg)](https://sqlite.org/)
[![License: AGPL v3](https://img.shields.io/badge/license-AGPL_v3-blue.svg)](LICENSE)

**Plus qu'un VCS : Une Forge de Développement Locale.**

> ⚠️ **Status:** Alpha / Experimental.

Silex est un système de contrôle de version nouvelle génération écrit en Rust. Contrairement à Git qui ne voit que des "snapshots" de fichiers, Silex suit des **Assets** et intègre directement dans votre dépôt les outils de gestion de projet (Chat, Todo, Analytics).

Le tout est propulsé par **SQLite**, ce qui rend votre historique et vos métadonnées 100% requêtables via SQL.

## 🚀 Pourquoi Silex ?

### 1. Philosophie "Asset-Centric"
Si vous renommez `main.rs` en `app.rs`, Git devine qu'il s'agit d'un renommage. Silex le **sait**. Chaque fichier possède un UUID unique (`asset_id`). L'historique suit l'identité du fichier, pas juste son chemin.

### 2. La "Forge" Intégrée
Pourquoi changer de fenêtre pour discuter ou noter une tâche ? Silex intègre ces outils directement dans le terminal, stockés localement dans le dépôt.
* **Messagerie Éphémère** : Laissez des notes aux collègues (ou à vous-même) qui s'autodétruisent à 20h00.
* **Todo List** : Gérez les tâches techniques directement là où se trouve le code.
* **Analytics** : Qui modifie quoi ? Quels fichiers sont liés ? Tout est dans la base SQL.

### 3. Puissance SQL
L'état de votre projet n'est pas caché dans des fichiers binaires obscurs. C'est une base de données.
```sql
-- Exemple : Trouver tous les fichiers modifiés par 'Saigo' pesant plus de 1MB
SELECT * FROM files WHERE author = 'Saigo' AND size > 1000000;

```

---

## 🛠 Installation

Prérequis : `Rust` (dernière version stable) et `libsqlite3`.

```bash
cargo install silex
```

### alias

Recommandé : Créer un alias

```bash
alias sx='silex'

```
---

## 💻 Utilisation

### Gestion de Version (VCS)

Les classiques, mais en mieux.

```bash
sx new            # Initialise un nouveau dépôt Silex (et la DB)
sx status         # Voir les changements
sx add .          # Stager les fichiers (Assets)
sx commit -m "feat: initial commit" 
sx log            # Voir l'historique

```

### Outils de Productivité (Nouveautés)

#### 💬 Chat Interne (Auto-destructible)

Idéal pour le "Daily standup" asynchrone ou les infos sensibles. Les messages disparaissent automatiquement à 20h.

```bash
sx chat send "Penser à refactoriser le module DB avant ce soir"
sx chat list      # Affiche les messages non expirés

```

#### ✅ Todo List

Plus besoin de `TODO:` perdus dans les commentaires du code.

```bash
sx todo add "Réparer le bug de la date" -u "Saigo" --due "2026-02-01"
sx todo list      # Affiche un joli tableau des tâches
sx todo close 42  # Termine la tâche ID 42

```

---

## 🏗 Architecture & Schéma

Le cœur de Silex repose sur deux bases SQLite dans `.silex/db/` :

1. **`store.db`** : Contient les `blobs` (contenu binaire dédupliqué via Blake3).
2. **`history_YYYY.db`** : Contient les métadonnées (Commits, Manifests, Chat, Todos).

### Tables Principales

* **`commits`** : Graphe des révisions (DAG).
* **`manifest`** : Table de liaison qui reconstruit le système de fichiers (`commit_id` + `asset_id` + `blob_id`).
* **`ephemeral_messages`** : Messages avec timestamp d'expiration.
* **`todos`** : Gestion des tâches avec assignation et dates limites.

---

## 🗺 Roadmap

* [x] **Core:** Structure Database & Init
* [x] **Productivity:** Chat & Todo System
* [x] **CLI:** Autocompletion (Fish) & UX with `tabled`
* [x] **VCS:** Checkout & Restore (Reconstruction des fichiers)
* [ ] **Sync:** Smart Sync (Diffs SQL uniquement)
* [ ] **Security:** Signature cryptographique des commits (Ed25519)

## 📄 Licence

Ce projet est sous licence **GNU Affero General Public License v3.0**. Voir le fichier [LICENSE](https://www.google.com/search?q=LICENSE) pour plus de détails.

