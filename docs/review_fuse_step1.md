# Code Review — FUSE Step 1 (sync-engine)

**Date:** 2026-03-26
**Reviewer:** Claude
**Scope:** Premier jet d'implémentation sync-engine/
**Build:** OK (3 warnings)
**Tests:** 14/14 pass

---

## Résumé

Bon premier jet. Les fondations sont solides : models alignés sur INTERFACES.md, bonne couverture de tests, architecture propre avec séparation VFS trait / FUSE backend / DB / services. Le code compile et les tests passent.

Les problèmes identifiés sont principalement des **connexions manquantes entre les briques** — chaque module fonctionne isolément mais le binary ne les assemble pas encore en un tout fonctionnel.

---

## Points forts

### Models (`src/models/`)
- FileState avec les 6 variantes canoniques, aligné INTERFACES.md
- `remote_id` présent dans FileNode
- `From<&str>` pour FileState avec fallback sur Error — bien pour le parsing DB
- `modified` en `String` ISO 8601 — évite le piège `OffsetDateTime::to_rfc3339()` qui n'existe pas

### VFS in-memory (`src/vfs/`)
- Séparation propre trait / implémentation
- `VfsError::InvalidTransition` prêt pour la state machine (pas encore utilisé)
- `FileMetadata` clean comme DTO pour `create_placeholder`
- 4 tests unitaires couvrant CRUD + hydration + error case

### Repository (`src/db/`)
- Mapping manuel SQLite → FileNode bien fait (pas de `query_as` qui ne marcherait pas avec nos types custom)
- `list_dir` par `parent_id` (subquery) au lieu du `LIKE '%'` cassé de la doc
- `INSERT OR REPLACE` — idempotent, bien pour le sync
- 3 tests avec `sqlite::memory:` — rapides et isolés

### FUSE backend (`src/fuse/`)
- Vraie API fuse3 0.8 avec les associated types `DirEntryStream` / `DirEntryPlusStream` — la doc ne les mentionnait pas, bien trouvé
- Mapping inode bidirectionnel (HashMap inode↔path + children)
- `mount_with_unprivileged` — pas besoin de sudo
- `make_attr` centralisé

### HydrationService (`src/services/`)
- Bon choix du générique `<V: VfsBackend>` avec `Arc<V>` au lieu du `Box<dyn VfsBackend>` non-clonable de la doc
- State machine Ghost → Syncing → Hydrated respectée
- `HydrationError` avec conversions `From` pour chaque type d'erreur sous-jacent
- 3 tests (happy path, already hydrated, not found)

---

## Problèmes à corriger

### P1 (bloquant) — Le binary ne monte pas le FUSE

**Fichier:** `src/bin/twake-vfs.rs`
**Lignes:** 21-44

Le binary crée un `InMemoryVfs`, ajoute un placeholder, puis boucle à l'infini dans un `sleep`. Le `TwakeFuseFs` et `mount_fuse()` ne sont jamais appelés. Le FUSE n'est pas monté.

**Attendu:**
```rust
use twake_sync::fuse::fuse_backend::TwakeFuseFs;
use twake_sync::fuse::mount_fuse;

let fs = TwakeFuseFs::new();
// ... register des nodes dans fs ...
let mount_handle = mount_fuse(fs, mount.to_str().unwrap()).await?;
info!("FUSE mounted at {:?}", mount);
mount_handle.await?; // bloque jusqu'à unmount
```

**Impact:** Sans ça, `ls ~/TwakeSync` ne montre rien. C'est le deliverable principal du MVP.

---

### P2 (bloquant) — `~/TwakeSync` n'est pas résolu

**Fichier:** `src/bin/twake-vfs.rs`
**Lignes:** 13, 31

`PathBuf::from("~/TwakeSync")` crée un chemin littéral `~/TwakeSync` — le `~` n'est pas expandé en Rust (c'est le shell qui fait ça). `create_dir_all("~/TwakeSync")` va créer un dossier appelé littéralement `~` dans le répertoire courant.

**Fix:**
```rust
fn expand_tilde(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~") {
        let home = std::env::var("HOME").expect("HOME not set");
        PathBuf::from(home).join(stripped)
    } else {
        path.to_path_buf()
    }
}

let mount = expand_tilde(&args.mount);
```

Aussi dans le `default_value` de clap, utiliser un chemin absolu ou documenter que `~` n'est pas supporté et demander le chemin complet.

---

### P3 (bloquant) — `TwakeFuseFs` et `InMemoryVfs` sont deux stores disjoints

**Fichiers:** `src/fuse/fuse_backend.rs`, `src/vfs/mod.rs`

Les deux structures maintiennent chacune leur propre `HashMap` de nodes. Créer un placeholder dans `InMemoryVfs` ne le rend pas visible dans le FUSE, et vice-versa. Ce sont deux univers parallèles.

**Options:**
1. **Option A (recommandé pour le MVP):** Utiliser `TwakeFuseFs` comme store unique. Le binary enregistre les nodes dans `TwakeFuseFs` via `register_node()`, et c'est FUSE qui les expose. `InMemoryVfs` reste pour les tests unitaires uniquement.

2. **Option B (plus propre, plus de travail):** Faire que `TwakeFuseFs` contienne un `Arc<dyn VfsBackend>` et délègue les lookups au VFS. Le mapping inode reste dans `TwakeFuseFs`, mais les données vivent dans le VFS.

L'option B est meilleure à terme car elle permet de brancher le Repository SQLite derrière le trait VfsBackend et d'avoir de la persistance. Mais pour un hackathon, l'option A suffit.

---

### P4 (important) — `open()` ne déclenche pas l'hydration

**Fichier:** `src/fuse/fuse_backend.rs`
**Lignes:** 129-135

```rust
async fn open(&self, _req: Request, inode: u64, flags: u32) -> FuseResult<ReplyOpen> {
    let nodes = self.nodes.read().await;
    if !nodes.contains_key(&inode) && inode != ROOT_INODE {
        return Err(libc::ENOENT.into());
    }
    Ok(ReplyOpen { fh: 0, flags })
}
```

C'est le comportement clé du MVP : ouvrir un ghost file doit déclencher le téléchargement. Actuellement, `open()` retourne OK même pour un Ghost, puis `read()` retourne `EIO`. L'utilisateur voit une erreur au lieu d'un téléchargement.

**Approches possibles:**

- **Sync blocking (simple, MVP):** Dans `open()`, si le fichier est Ghost, déclencher le download de manière synchrone (bloquer le `open` jusqu'à ce que le fichier soit hydraté). L'app qui ouvre le fichier attend simplement un peu plus longtemps. C'est ce que fait OneDrive sur Windows.

- **Async avec retry (plus complexe):** Retourner `EAGAIN` sur `open()` pour les Ghosts, lancer l'hydration en background. L'app réessaie automatiquement. Moins propre, pas tous les programmes gèrent bien `EAGAIN` sur `open`.

Pour le MVP, l'approche sync blocking est la bonne. `TwakeFuseFs` aura besoin d'une référence vers le `HydrationService` (ou un channel pour lui envoyer des requêtes).

---

### P5 (important) — `read()` ne lit pas de contenu réel

**Fichier:** `src/fuse/fuse_backend.rs`
**Lignes:** 137-148

```rust
async fn read(&self, ...) -> FuseResult<ReplyData> {
    // ...
    Ok(ReplyData { data: Bytes::new() })
}
```

Même pour un fichier hydraté, `read()` retourne des bytes vides. Il faut un **cache directory** où stocker les contenus téléchargés, et lire depuis ce cache.

**Architecture proposée:**
```
~/.twake/cache/
├── 550e8400-e29b-41d4-a716-446655440000    # contenu du fichier, nommé par UUID
├── 6f1a3b2c-...
└── ...
```

Dans `read()`:
```rust
let cache_path = self.cache_dir.join(node.id.to_string());
let data = tokio::fs::read(&cache_path).await?;
// appliquer offset + size
let slice = &data[offset as usize..min(offset as usize + size as usize, data.len())];
Ok(ReplyData { data: Bytes::copy_from_slice(slice) })
```

Et dans `HydrationService::download_file()`, écrire dans le cache dir au lieu du chemin FUSE.

---

### P6 (mineur) — Ghost files affichent 0 bytes

**Fichier:** `src/fuse/fuse_backend.rs`
**Ligne:** 49

```rust
let size = if node.state == FileState::Ghost { 0 } else { node.size };
```

Les ghost files apparaissent comme des fichiers de 0 bytes dans `ls -l`. C'est techniquement correct, mais :
- Des gestionnaires de fichiers peuvent masquer les fichiers vides
- L'utilisateur ne sait pas quelle est la "vraie" taille avant download
- OneDrive et Dropbox Smart Sync affichent la taille réelle même pour les fichiers non téléchargés

**Recommandation:** Afficher `node.size` (la taille metadata du remote) pour tous les états. Ça donne une meilleure UX sans impact fonctionnel.

---

### P7 (mineur) — 3 imports inutilisés

```
warning: unused import: `sqlx::FromRow`
 --> src/models/file_node.rs:2:5

warning: unused import: `Stream`
  --> src/fuse/fuse_backend.rs:10:34

warning: unused import: `std::path::Path`
 --> src/fuse/mod.rs:5:5
```

Supprimer les 3 lignes. Un `cargo fix --lib` les supprime automatiquement.

---

## Suggestions pour la suite (step 2)

### Ordre recommandé

1. **Fixer P1+P2** — le binary monte le FUSE pour de vrai
2. **Fixer P3** — `TwakeFuseFs` comme store unique (option A)
3. **Ajouter le cache dir** — `~/.twake/cache/` (résout P5)
4. **Brancher l'hydration dans `open()`** (résout P4)
5. **Tester end-to-end:** `cargo run -- --mount /tmp/test-fuse && ls /tmp/test-fuse`

### Architecture cible step 2

```
twake-vfs (binary)
    │
    ├── TwakeFuseFs (FUSE filesystem, store principal)
    │   ├── nodes: HashMap<inode, FileNode>
    │   ├── children: HashMap<inode, Vec<inode>>
    │   └── cache_dir: PathBuf (~/.twake/cache/)
    │
    ├── HydrationService
    │   ├── CozyClient (download depuis Cozy)
    │   └── cache_dir: PathBuf (même que FUSE)
    │
    └── FileRepository (SQLite, persistance entre restarts)
```

**Flow open() sur un Ghost:**
```
User: cat ~/TwakeSync/doc.txt
  → FUSE open(inode=42)
    → node.state == Ghost
    → HydrationService.hydrate(node)
      → CozyClient.download(node.remote_id)
      → write to ~/.twake/cache/{uuid}
      → node.state = Hydrated
    → return Ok(ReplyOpen)
  → FUSE read(inode=42, offset=0, size=4096)
    → read from ~/.twake/cache/{uuid}
    → return ReplyData
```

---

## Checklist de validation step 1

- [x] `cargo build` : OK (3 warnings mineurs)
- [x] `cargo test` : 14/14 pass
- [x] FileState 6 variantes : OK
- [x] FileNode avec remote_id : OK
- [x] DB schema avec remote_id : OK
- [x] VFS trait avec tests : OK
- [x] FUSE backend compile avec fuse3 0.8 : OK
- [x] Repository CRUD avec tests : OK
- [x] HydrationService avec tests : OK
- [ ] Binary monte le FUSE : **NON** (P1)
- [ ] Fichiers visibles dans le mount : **NON** (P1+P3)
- [ ] Open ghost → hydrate : **NON** (P4)
- [ ] Read retourne du contenu : **NON** (P5)
