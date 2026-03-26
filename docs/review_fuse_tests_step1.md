# Code Review — Tests FUSE Step 1

**Date:** 2026-03-26
**Reviewer:** Claude
**Scope:** Couverture de tests de sync-engine/
**Tests:** 14/14 pass
**Verdict:** Happy paths unitaires OK, edge cases et intégration absents

---

## Cartographie de couverture

| Module | Fichier | Tests | Couverture |
|---|---|---|---|
| models/file_state | `models/mod.rs` | 2 | Display + From<&str> |
| models/file_node | `models/mod.rs` | 2 | Constructeur + serde JSON |
| models/file_status | — | **0** | **Non testé** |
| vfs (InMemoryVfs) | `vfs/mod.rs` | 4 | CRUD + hydrate + list + not_found |
| db/repository | `db/repository.rs` | 3 | CRUD + update_state + list_dir |
| services/hydration | `services/hydration.rs` | 3 | Happy path + already hydrated + not_found |
| fuse/fuse_backend | — | **0** | **Non testé** |
| fuse/mount | — | **0** | **Non testé** |
| bin/twake-vfs | — | **0** | **Non testé** |

**Résumé:** 14 tests, 5 modules testés sur 9. Les deux modules les plus critiques (FUSE backend et binary) ont zéro test.

---

## Bugs cachés non détectés par les tests

### BUG-1 — `InMemoryVfs::list_dir` matche trop large (sévérité: haute)

**Fichier:** `src/vfs/mod.rs:46-48`

```rust
Ok(nodes.values()
    .filter(|n| n.path.starts_with(prefix))
    .cloned()
    .collect())
```

`String::starts_with` ne respecte pas les frontières de répertoire. Un appel à `list_dir("/test")` retourne aussi les fichiers de `/testing/`, `/test-old/`, `/testament/`, etc.

**Preuve:**
```rust
"/testing/file.txt".starts_with("/test")  // → true (BUG)
"/test-old/notes.md".starts_with("/test") // → true (BUG)
```

**Le test actuel ne le détecte pas** parce qu'il utilise `/test` et `/other` — deux préfixes qui ne se chevauchent pas.

**Test qui piégerait le bug:**
```rust
#[tokio::test]
async fn test_list_dir_does_not_match_sibling_prefix() {
    let vfs = InMemoryVfs::new();

    vfs.create_placeholder(
        Path::new("/test/file1.txt"),
        FileMetadata { size: 100, modified: "2026-01-01T00:00:00Z".into(), is_dir: false },
    ).await.unwrap();

    vfs.create_placeholder(
        Path::new("/testing/file2.txt"),
        FileMetadata { size: 200, modified: "2026-01-01T00:00:00Z".into(), is_dir: false },
    ).await.unwrap();

    let files = vfs.list_dir(Path::new("/test")).await.unwrap();
    assert_eq!(files.len(), 1); // FAIL avec l'implémentation actuelle: retourne 2
}
```

**Fix:** Ajouter un `/` au préfixe ou filtrer par `parent_id` :
```rust
let prefix = format!("{}/", path.to_str().unwrap().trim_end_matches('/'));
Ok(nodes.values()
    .filter(|n| n.path.starts_with(&prefix))
    .cloned()
    .collect())
```

---

### BUG-2 — L'hydration ne rollback pas en cas d'échec download (sévérité: haute)

**Fichier:** `src/services/hydration.rs:22-41`

```rust
pub async fn hydrate_file(&self, path: &Path) -> Result<(), HydrationError> {
    // ...
    self.vfs.set_state(path, FileState::Syncing).await?;

    self.download_file(path, &node).await?;  // ← si ça échoue ici...

    self.vfs.set_state(path, FileState::Hydrated).await?;
    // ...
}
```

Si `download_file` échoue (réseau, disque plein, etc.), la fonction retourne une erreur mais le node reste en état `Syncing` pour toujours. Aucune tentative de remise en `Ghost` ou passage en `Error`.

**Conséquence:** Le fichier est bloqué — ni Ghost (donc pas re-téléchargeable), ni Hydrated (donc pas lisible), ni Error (donc pas détecté par le monitoring).

**Aucun test ne couvre ce scénario.** Les 3 tests d'hydration sont :
1. Happy path (download OK)
2. Already hydrated (skip)
3. Fichier inexistant (erreur avant d'atteindre le download)

Aucun ne teste un download qui échoue après le passage en Syncing.

**Test manquant:**
```rust
#[tokio::test]
async fn test_hydrate_download_failure_rollbacks_state() {
    // Setup: créer un ghost file dans le VFS
    // Injecter un mock/VFS qui fait échouer le download
    // Appeler hydrate_file
    // Vérifier que le state est Error (ou Ghost), PAS Syncing
}
```

**Fix dans hydration.rs:**
```rust
self.vfs.set_state(path, FileState::Syncing).await?;

if let Err(e) = self.download_file(path, &node).await {
    // Rollback: marquer en erreur plutôt que laisser en Syncing
    let _ = self.vfs.set_state(path, FileState::Error).await;
    let _ = self.repo.update_state(path.to_str().unwrap(), FileState::Error).await;
    return Err(e);
}

self.vfs.set_state(path, FileState::Hydrated).await?;
```

---

### BUG-3 — L'hydration ne synchronise pas la DB (sévérité: moyenne)

**Fichier:** `src/services/hydration.rs`

Le test `test_hydrate_ghost_file` crée un placeholder dans le VFS (`vfs.create_placeholder`), hydrate le fichier, puis vérifie `vfs.get_state()`. Mais :

1. Le node n'est **jamais inséré dans la DB** (pas de `repo.insert()`)
2. `hydrate_file` appelle `self.repo.update_state()` sur un node qui n'existe pas en DB
3. Le test ne vérifie **pas** que la DB est à jour après hydration

**Le test passe** parce que `update_state` sur un row inexistant ne retourne pas d'erreur SQL (UPDATE 0 rows = OK). Mais en production, la DB et le VFS seraient désynchronisés.

**Test manquant:**
```rust
#[tokio::test]
async fn test_hydrate_updates_db_state() {
    let vfs = Arc::new(InMemoryVfs::new());
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let repo = FileRepository { pool: pool.clone() };
    let service = HydrationService::new(vfs.clone(), repo);

    let path = Path::new("/test/file.txt");

    // Créer dans le VFS ET dans la DB
    let mut node = FileNode::new_ghost("/test/file.txt", false);
    node.modified = "2026-01-01T00:00:00Z".to_string();
    vfs.create_placeholder(path, FileMetadata { ... }).await.unwrap();

    let db_repo = FileRepository { pool };
    db_repo.insert(&node).await.unwrap();

    // Hydrate (utilise un temp dir pour le vrai fichier)
    let temp_path = std::env::temp_dir().join("test_db_sync.txt");
    service.hydrate_file(&temp_path).await.unwrap();

    // Vérifier que la DB reflète Hydrated
    let db_node = db_repo.get("/test/file.txt").await.unwrap().unwrap();
    assert_eq!(db_node.state, FileState::Hydrated);  // Probablement FAIL
}
```

---

## Modules sans aucun test

### FUSE backend (`src/fuse/fuse_backend.rs`) — 0 tests

C'est le module le plus critique du projet et il n'a aucun test. Les méthodes du trait `Filesystem` sont testables directement sans monter un vrai FUSE — il suffit de forger des `Request` et d'appeler les méthodes.

**Tests essentiels manquants:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_fs() -> TwakeFuseFs {
        let fs = TwakeFuseFs::new();
        let node = FileNode {
            id: uuid::Uuid::new_v4(),
            remote_id: Some("cozy-123".to_string()),
            path: "/documents/test.txt".to_string(),
            state: FileState::Ghost,
            size: 1024,
            modified: "2026-01-01T00:00:00Z".to_string(),
            is_dir: false,
            parent_id: None,
        };
        fs.register_node(node, ROOT_INODE).await;
        fs
    }

    // T1: register_node attribue un inode unique
    #[tokio::test]
    async fn test_register_node_assigns_unique_inode() { ... }

    // T2: lookup trouve un enfant par nom
    #[tokio::test]
    async fn test_lookup_finds_child_by_name() { ... }

    // T3: lookup retourne ENOENT pour un nom inexistant
    #[tokio::test]
    async fn test_lookup_returns_enoent_for_missing() { ... }

    // T4: getattr sur root retourne Directory
    #[tokio::test]
    async fn test_getattr_root_is_directory() { ... }

    // T5: getattr sur un fichier retourne les bons attributs
    #[tokio::test]
    async fn test_getattr_file_returns_correct_attrs() { ... }

    // T6: readdir sur root liste les enfants + . et ..
    #[tokio::test]
    async fn test_readdir_lists_children() { ... }

    // T7: readdir avec offset skip les premières entrées
    #[tokio::test]
    async fn test_readdir_respects_offset() { ... }

    // T8: read sur un Ghost retourne EIO
    #[tokio::test]
    async fn test_read_ghost_returns_eio() { ... }

    // T9: getattr sur inode inexistant retourne ENOENT
    #[tokio::test]
    async fn test_getattr_unknown_inode_returns_enoent() { ... }
}
```

**Note:** Forger un `fuse3::Request` nécessite de vérifier si le type est constructible publiquement. Si ce n'est pas le cas, il faudra soit un wrapper de test, soit tester via un trait maison qui abstrait les appels FUSE.

---

### FileStatus (`src/models/file_status.rs`) — 0 tests

La conversion `From<&FileNode>` est triviale mais c'est le type qui passe sur le fil IPC. Si un champ est ajouté à `FileNode` sans mettre à jour `FileStatus`, rien ne le détecte.

**Test manquant:**
```rust
#[test]
fn test_file_status_from_file_node() {
    let node = FileNode {
        id: uuid::Uuid::new_v4(),
        remote_id: Some("abc".to_string()),
        path: "/test.txt".to_string(),
        state: FileState::Ghost,
        size: 42,
        modified: "2026-03-26T10:00:00Z".to_string(),
        is_dir: false,
        parent_id: None,
    };

    let status = FileStatus::from(&node);

    assert_eq!(status.path, "/test.txt");
    assert_eq!(status.state, FileState::Ghost);
    assert_eq!(status.size, 42);
    assert_eq!(status.modified, "2026-03-26T10:00:00Z");
}
```

---

## Tests existants : points d'attention

### Repository : `unwrap()` sur les `Uuid::parse_str`

**Fichier:** `src/db/repository.rs:26, 33, 77, 84`

```rust
id: Uuid::parse_str(&r.get::<String, _>("id")).unwrap(),
```

Si la DB contient une valeur corrompue dans la colonne `id`, c'est un panic en production. Aucun test ne vérifie la robustesse face à des données DB invalides. C'est acceptable pour un hackathon, mais à garder en tête.

### Repository : pas de test d'upsert

`INSERT OR REPLACE` est utilisé, mais aucun test ne vérifie qu'un `insert` sur un path existant met à jour au lieu de dupliquer. Test manquant :

```rust
#[tokio::test]
async fn test_insert_replaces_existing() {
    // insert node avec path "/test.txt" et size 100
    // insert un autre node avec même path mais size 200
    // get("/test.txt") → size devrait être 200
    // vérifier qu'il n'y a qu'une seule row
}
```

### Repository : `list_dir` ne teste pas l'exclusion des petits-enfants

Le test crée un parent avec 2 enfants directs. Mais un petit-enfant dont le `parent_id` pointe vers un enfant (et non le parent) serait-il correctement exclu ? La requête SQL est correcte (filtre par parent_id), mais le test ne le prouve pas.

```rust
#[tokio::test]
async fn test_list_dir_excludes_grandchildren() {
    // parent /test (id=AAA)
    //   child /test/sub (id=BBB, parent_id=AAA)
    //     grandchild /test/sub/file.txt (id=CCC, parent_id=BBB)
    // list_dir("/test") → devrait retourner [sub] seulement, pas [sub, file.txt]
}
```

### VFS : pas de test concurrent

L'`InMemoryVfs` utilise `RwLock` pour la thread safety, mais aucun test ne vérifie que des accès concurrents (create + list simultanés, hydrate pendant un read) ne causent pas de deadlock ou de corruption.

---

## Tests d'intégration manquants

Aucun test ne traverse plusieurs modules. Chaque module est testé en isolation avec ses propres mocks/setup. Ça masque les problèmes de branchement (comme P3 — les deux stores disjoints).

**Tests d'intégration prioritaires:**

### I1 — Repository → VFS coherence
```rust
// Insérer un node en DB, le retrouver via VFS (ou l'inverse)
// Vérifier que les deux stores ont la même vue
```

### I2 — Hydration end-to-end
```rust
// 1. Insérer node Ghost dans DB ET VFS
// 2. Appeler hydrate_file
// 3. Vérifier state Hydrated dans DB ET VFS
// 4. Vérifier qu'un fichier existe sur disque
```

### I3 — FUSE → Hydration flow
```rust
// 1. Enregistrer un Ghost dans TwakeFuseFs
// 2. Appeler open() → doit déclencher hydration
// 3. Appeler read() → doit retourner du contenu
// (nécessite que P4 soit fixé)
```

---

## Matrice de risque

| Zone | Tests existants | Risque sans tests | Priorité |
|---|---|---|---|
| list_dir prefix bug | Aucun | **Arbre de fichiers cassé dès noms similaires** | P0 |
| Hydration rollback | Aucun | **Fichiers bloqués en Syncing** | P0 |
| FUSE lookup/readdir | Aucun | **Rien ne s'affiche dans le mount** | P1 |
| FUSE open/read | Aucun | **Fichiers inaccessibles** | P1 |
| DB/VFS sync | Aucun | **Désync silencieuse entre persistence et runtime** | P2 |
| FileStatus conversion | Aucun | **Réponses IPC incorrectes** | P3 |
| Upsert repository | Aucun | **Duplicats en DB sur re-sync** | P3 |
| Concurrent access | Aucun | **Deadlocks potentiels sous charge** | P3 |

---

## Recommandation

Pour le hackathon, fixer en priorité :

1. **Le bug list_dir** (BUG-1) — c'est un vrai bug, pas un test manquant
2. **Le rollback hydration** (BUG-2) — fichiers zombie en Syncing sinon
3. **Tests FUSE de base** (T1-T6) — c'est le deliverable, il faut un minimum de confiance
4. **Un test d'intégration** (I2) — pour valider que les briques se branchent

Le reste (upsert, concurrent, FileStatus) peut attendre post-hackathon.
