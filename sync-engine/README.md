# Twake Sync Engine

Moteur de synchronisation FUSE pour Twake Drive. Monte un systeme de fichiers virtuel qui expose l'arborescence d'une instance Cozy et telecharge les fichiers a la demande (hydratation on-demand).

## Principe

```
  Cozy Cloud (io.cozy.files)
        |
        | HTTPS (JSON:API)
        v
  +------------------+
  |   CozyClient     |  list_dir(), download()
  +------------------+
        |
        v
  +------------------+
  |  TwakeFuseFs     |  FUSE filesystem (fuse3, unprivileged)
  |                  |  - Inode table en memoire
  |                  |  - Cache disque (~/.twake/cache/)
  +------------------+
        |
        | FUSE (kernel)
        v
  ~/twake-drive/     <-- point de montage utilisateur
```

1. Au lancement, `twake-vfs` liste recursivement les fichiers depuis Cozy et peuple l'arborescence FUSE avec des **ghost files** (metadata uniquement, 0 octets sur disque).
2. Quand un fichier est ouvert (`open()`), le contenu est telecharge depuis Cozy et ecrit dans le cache local.
3. Les lectures suivantes (`read()`) sont servies depuis le cache.

## Prerequis

- **Rust** >= 1.75 (edition 2021)
- **FUSE 3** (libfuse3-dev) :
  ```bash
  # Debian/Ubuntu
  sudo apt install fuse3 libfuse3-dev pkg-config

  # Fedora
  sudo dnf install fuse3-devel
  ```
- **SQLite** (inclus via sqlx, pas de dep systeme)
- L'utilisateur doit etre dans le groupe `fuse` ou avoir les permissions unprivileged FUSE :
  ```bash
  # Verifier
  grep fuse /etc/group
  # Ajouter si necessaire
  sudo usermod -aG fuse $USER
  ```

## Build

```bash
cd sync-engine
cargo build --release
```

Le binaire est produit dans `target/release/twake-vfs`.

## Tests

```bash
cargo test
```

32 tests unitaires couvrent : models, VFS in-memory, hydration service, repository SQLite, et le backend FUSE (register_node, lookup, getattr, readdir, open/hydrate, read avec offsets).

## Utilisation

### 1. Obtenir un token Cozy

Depuis le navigateur, connecte-toi a ton instance Cozy Drive, puis :

1. Ouvre les **DevTools** (F12) > onglet **Network**
2. Navigue dans un dossier pour generer des requetes
3. Filtre par `files` et clique sur une requete XHR
4. Copie le header `Authorization: Bearer eyJ...`

Si ton instance est derriere un SSO (LemonLDAP, etc.), tu auras aussi besoin des cookies de session. Dans DevTools > Application > Cookies, copie les valeurs des cookies `sess-cozy*` et `lemonldap`.

### 2. Monter le filesystem

```bash
# Montage de base (token seul)
./target/release/twake-vfs \
  --mount ~/twake-drive \
  --url "https://mon-instance.cozy.cloud" \
  --token "eyJ..."

# Avec cookies de session (instances derriere SSO)
./target/release/twake-vfs \
  --mount ~/twake-drive \
  --url "https://pvi.stg.lin-saas.com" \
  --token "eyJ..." \
  --cookie "sess-cozyXXX=AAA...; lemonldap=bbb..."
```

Options :

| Option | Description |
|--------|-------------|
| `--mount`, `-m` | Point de montage (sera cree si absent) |
| `--url`, `-u` | URL du stack Cozy (pas l'app Drive) |
| `--token`, `-t` | Bearer token JWT |
| `--cookie` | Cookies de session (optionnel, pour SSO) |
| `--cache`, `-c` | Repertoire de cache (defaut: `~/.twake/cache/`) |

### 3. Utiliser

```bash
# Lister les fichiers
ls ~/twake-drive/

# Lire un fichier (declenche le telechargement)
cat ~/twake-drive/Documents/rapport.pdf

# Copier un fichier depuis le cloud
cp ~/twake-drive/Photos/image.png ~/Bureau/
```

### 4. Demonter

```bash
fusermount3 -u ~/twake-drive
```

### Logs

```bash
# Logs standard
RUST_LOG=info ./target/release/twake-vfs ...

# Logs detailles (requetes HTTP, etc.)
RUST_LOG=debug ./target/release/twake-vfs ...
```

## Architecture des sources

```
src/
  bin/twake-vfs.rs    CLI, point d'entree
  cozy/
    client.rs         Client HTTP Cozy (list_dir, download)
  fuse/
    fuse_backend.rs   Implementation FUSE (Filesystem trait)
    mod.rs            Montage FUSE (Session, MountOptions)
  models/
    file_node.rs      FileNode (id, path, state, size, ...)
    file_state.rs     Ghost | Hydrated | Modified | Syncing | ...
    file_status.rs    Snapshot de statut
  vfs/
    vfs_trait.rs      Trait VfsBackend
    mod.rs            InMemoryVfs
  db/
    repository.rs     CRUD SQLite (FileRepository)
  services/
    hydration.rs      Service d'hydratation
```

## Limitations (MVP)

- **Lecture seule** : pas de write, mkdir, rename, unlink
- **Pas de sync continue** : l'arborescence est chargee au lancement, pas de change feed
- **Token ephemere** : le JWT expire avec la session, pas de refresh automatique
- **Pas de persistance FUSE -> DB** : les deux stores sont disjoints
