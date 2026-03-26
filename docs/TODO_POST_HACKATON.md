# TODO Post-Hackathon — FUSE Sync Engine

> Ce document liste les fonctionnalités, corrections et améliorations reportées après le hackathon MVP.

---

## 🚨 Bugs à corriger (priorité haute)

### BUG-1 — `list_dir` matche trop large
**Sévérité:** P0  
**Fichier:** `src/vfs/mod.rs:46-48`

```rust
// BUG: "/testing/file.txt".starts_with("/test") retourne true
.filter(|n| n.path.starts_with(prefix))

// FIX:
let prefix = format!("{}/", path.to_str().unwrap().trim_end_matches('/'));
.filter(|n| n.path.starts_with(&prefix))
```

---

### BUG-2 — Hydration ne rollback pas en cas d'échec
**Sévérité:** P0  
**Fichier:** `src/services/hydration.rs:22-41`

Si `download_file()` échoue après passage en `Syncing`, le fichier reste bloqué en `Syncing` pour toujours (ni Ghost, ni Hydrated, ni Error).

**FIX:** Ajouter le rollback sur erreur:
```rust
self.vfs.set_state(path, FileState::Syncing).await?;

if let Err(e) = self.download_file(path, &node).await {
    // Rollback: marquer en erreur
    let _ = self.vfs.set_state(path, FileState::Error).await;
    let _ = self.repo.update_state(path.to_str().unwrap(), FileState::Error).await;
    return Err(e);
}

self.vfs.set_state(path, FileState::Hydrated).await?;
```

---

### BUG-3 — Hydration ne synchronise pas la DB
**Sévérité:** P2  
**Fichier:** `src/services/hydration.rs`

**Status:** ✅ CORRIGÉ dans le code - `repo.update_state()` est bien appelé après hydratation (ligne 41).
**Restant:** Ajouter un test d'intégration pour vérifier la cohérence DB ↔ VFS.

---

## 🔧 Fonctionnalités incomplètes (step 2)

### P1 — Binary ne monte pas le FUSE
**Status:** Step 1 non terminé  
**Fichier:** `src/bin/twake-vfs.rs`

Le binary fait juste un sleep infinito, il ne monte pas le FUSE.

**À faire:**
```rust
let fs = TwakeFuseFs::new();
// ... register des nodes ...
let mount_handle = mount_fuse(fs, mount.to_str().unwrap()).await?;
mount_handle.await?; // bloque jusqu'à unmount
```

---

### P2 — `~/TwakeSync` non résolu
**Status:** Step 1 non terminé  
**Fichier:** `src/bin/twake-vfs.rs`

`PathBuf::from("~/TwakeSync")` crée un chemin littéral, pas le home directory.

**FIX:** Utiliser `dirs::home()` ou implémenter expand tilde.

---

### P3 — TwakeFuseFs et InMemoryVfs sont disjoints
**Status:** Step 1 non terminé

Deux stores séparés avec leurs propres HashMaps. Choisir l'architecture cible:

- **Option A (MVP):** TwakeFuseFs comme store unique
- **Option B (Production):** TwakeFuseFs délègue à un `Arc<dyn VfsBackend>` branché sur SQLite

---

### P4 — `open()` ne déclenche pas l'hydration
**Status:** Step 1 non terminé  
**Fichier:** `src/fuse/fuse_backend.rs`

Ouvrir un Ghost file devrait déclencher le download.

**Approche recommandée:** Sync blocking avec timeout pour le MVP.

---

### P5 — `read()` retourne toujours vide
**Status:** Step 1 non terminé  
**Fichier:** `src/fuse/fuse_backend.rs`

Implémenter un cache directory `~/.twake/cache/` et lire depuis ce cache dans `read()`.

---

### P6 — Ghost files affichent 0 bytes
**Status:** En discussion (objection)  
**Fichier:** `src/fuse/fuse_backend.rs`

Option recommandée: afficher la vraie taille (metadata du remote) mais ajouter un marqueur cloud (autrement que juste la taille).

---

## 🧪 Couverture de tests manquante

### Tests FUSE backend
**Priorité:** P1  
**Fichiers:** `src/fuse/fuse_backend.rs`

Ajouter tests pour:
- `register_node` attribue un inode unique
- `lookup` trouve les enfants
- `getattr` retourne les bons attributs
- `readdir` liste correctement
- `read` sur Ghost retourne EIO

---

### Tests FileStatus
**Priorité:** P3  
**Fichier:** `src/models/file_status.rs`

Test de non-régression pour la conversion `From<&FileNode>`.

---

### Tests de concurrence VFS
**Priorité:** P3  
**Fichier:** `src/vfs/mod.rs`

Vérifier que l'accès concurrent (create + list simultanés) ne cause pas de deadlock.

---

### Tests d'intégration

#### I1 — Repository ↔ VFS coherence
Insérer un node en DB, le retrouver via VFS, vérifier les deux stores sont synchronisés.

#### I2 — Hydration end-to-end
1. Insérer Ghost en DB + VFS
2. Appeler hydrate_file
3. Vérifier state Hydrated en DB ET VFS
4. Vérifier fichier existe sur disque

#### I3 — FUSE → Hydration flow
1. Enregistrer Ghost dans TwakeFuseFs
2. Appeler open() → doit déclencher hydration
3. Appeler read() → doit retourner du contenu

---

## 🧹 Nettoyage

### P7 — Imports inutilisés
**Priorité:** Faible

```bash
cargo fix --lib
```

Supprimer:
- `sqlx::FromRow` dans `src/models/file_node.rs`
- `Stream` dans `src/fuse/fuse_backend.rs`
- `std::path::Path` dans `src/fuse/mod.rs`

---

### Repository — Tests de robustesse
- Test d'upsert (insert sur existing path met à jour)
- Test d'exclusion des petits-enfants dans list_dir
- Gestion des UUID corrompus en DB (actuellement `unwrap()`)

---

## 📋 Architecture cible

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
    └── FileRepository (SQLite, persistance)
```

---

## 📅 Ordre recommandé

1. **Fixer les bugs** (BUG-1, BUG-2) — critiques pour la stabilité
2. **Terminer le step 1** (P1-P5) — faire fonctionner le FUSE mount
3. **Ajouter tests d'intégration** (I2) — valider que les briques se branchent
4. **Compléter la couverture de tests** — FUSE backend, FileStatus
5. **Améliorations UX** — P6, marqueurs cloud

---

*Document généré automatiquement suite au code review FUSE Step 1 (2026-03-26)*