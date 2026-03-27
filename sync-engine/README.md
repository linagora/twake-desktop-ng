# Twake Sync Engine

Moteur de synchronisation FUSE pour Twake Drive. Monte un systeme de fichiers virtuel qui expose l'arborescence d'une instance Cozy avec synchronisation bidirectionnelle en temps reel.

## Principe

```
  Cozy Cloud (io.cozy.files)
        |
        | HTTPS (JSON:API) + WebSocket (realtime)
        v
  +------------------+
  |   CozyClient     |  list_dir(), download(), upload(), delete(), move()
  +------------------+
        |
        v
  +------------------+
  |  TwakeFuseFs     |  FUSE filesystem (fuse3, unprivileged)
  |                  |  - Inode table en memoire
  |                  |  - Cache disque (~/.twake/cache/)
  |                  |  - Upload queue async
  +------------------+
        |
        | FUSE (kernel)
        v
  ~/TwakeSync/       <-- point de montage utilisateur
```

1. Au lancement, `twake-vfs` liste recursivement les fichiers depuis Cozy et peuple l'arborescence FUSE avec des **ghost files** (metadata uniquement, 0 octets sur disque).
2. Quand un fichier est ouvert (`open()`), le contenu est telecharge depuis Cozy et ecrit dans le cache local.
3. Les lectures suivantes (`read()`) sont servies depuis le cache.
4. Les ecritures (`write()`, `create()`, `mkdir()`) sont propagees au serveur via une upload queue asynchrone.
5. Les suppressions (`rm`, `rmdir`) sont propagees au serveur en temps reel.
6. Les changements cote serveur sont detectes via **WebSocket realtime** (< 1s) avec un **polling fallback** configurable.

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

81 tests unitaires couvrent : models, VFS in-memory, hydration service, deconfliction, repository SQLite, et le backend FUSE (register_node, lookup, getattr, setattr, access, readdir, open/hydrate, read avec offsets, create/write/flush a la racine et en sous-dossier, mkdir, rmdir, unlink, rename, sync bidirectionnel).

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
  --mount ~/TwakeSync \
  --url "https://mon-instance.cozy.cloud" \
  --token "eyJ..."

# Avec cookies de session (instances derriere SSO)
./target/release/twake-vfs \
  --mount ~/TwakeSync \
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
| `--sync-interval` | Intervalle de polling fallback en secondes (defaut: 60) |

### 3. Utiliser

```bash
# Lister les fichiers
ls ~/TwakeSync/

# Lire un fichier (declenche le telechargement)
cat ~/TwakeSync/Documents/rapport.pdf

# Copier un fichier depuis le cloud
cp ~/TwakeSync/Photos/image.png ~/Bureau/

# Creer un fichier (propage vers le serveur)
echo "hello" > ~/TwakeSync/nouveau.txt
touch ~/TwakeSync/vide.txt

# Creer un dossier
mkdir ~/TwakeSync/MonDossier

# Supprimer (propage vers le serveur)
rm ~/TwakeSync/nouveau.txt
rmdir ~/TwakeSync/MonDossier

# Renommer / deplacer
mv ~/TwakeSync/ancien.txt ~/TwakeSync/nouveau.txt
```

### 4. Demonter

```bash
fusermount3 -u ~/TwakeSync
```

### Logs

```bash
# Logs standard
RUST_LOG=info ./target/release/twake-vfs ...

# Logs detailles (requetes HTTP, WebSocket, etc.)
RUST_LOG=debug ./target/release/twake-vfs ...
```

## Architecture des sources

```
src/
  bin/twake-vfs.rs    CLI, point d'entree, sync loop + realtime
  cozy/
    client.rs         Client HTTP Cozy (list, download, upload, update, delete, mkdir, move)
    realtime.rs       Client WebSocket Cozy (io.cozy.files events)
  fuse/
    fuse_backend.rs   Implementation FUSE (Filesystem trait) + SharedVfsState
    mod.rs            Montage FUSE (Session, MountOptions)
  models/
    file_node.rs      FileNode (id, path, state, size, ...)
    file_state.rs     Ghost | Hydrated | Modified | Syncing | Synced | ...
    file_status.rs    Snapshot de statut
  vfs/
    vfs_trait.rs      Trait VfsBackend
    mod.rs            InMemoryVfs
  db/
    repository.rs     CRUD SQLite (FileRepository)
  services/
    hydration.rs      Service d'hydratation
    upload_queue.rs   Queue d'upload async avec linking remote_id
    deconfliction.rs  Gestion des conflits de noms
```

## Limitations actuelles

- **Token ephemere** : le JWT expire avec la session, pas de refresh automatique
- **Pas de persistance FUSE -> DB** : les deux stores sont disjoints
- **Pas de sync incrementale** : le polling re-liste l'arborescence complete (le WebSocket compense en temps reel)
