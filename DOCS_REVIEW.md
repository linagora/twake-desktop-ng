# Documentation Review - Twake Desktop NG

**Date:** 2026-03-26
**Reviewer:** Claude (audit automatise)
**Scope:** Ensemble de la documentation projet

---

## 1. Structure et organisation

**Points forts :**
- Hierarchie claire : spec -> design specs -> plans -> ADR
- Le `docs/spec.md` (v5.0 Final) sert bien de document chapeau qui pointe vers les specs detaillees
- Les ADR sont bien structures avec un template et un index

**Problemes identifies :**

- **Cohabitation `spec.md` (v5.0) et `spec-draft.md` (v2.0)** — Le draft contient du contenu important absent de la spec finale (matrice CEF/Electron detaillee, analyse des risques, section "What to Avoid", architecture notification). On ne sait pas s'il est obsolete ou complementaire. Il mentionne encore Tauri dans son tableau d'avantages techniques (ligne 293 : "Tauri + Rust -- 10MB install, 30MB RAM") alors que CEF a ete choisi.

- **Documents racine vs docs/** — Les fichiers `PLAN.md`, `STREAM_*.md`, `INTERFACES.md` sont a la racine tandis que les specs sont dans `docs/superpowers/specs/`. L'ADR-0001 documente cette decision, mais le resultat est un eclatement sur 3 niveaux (racine, docs/, docs/superpowers/).

- **Pas de README.md a la racine** — Aucun point d'entree pour un nouveau developpeur.

---

## 2. Coherence des donnees entre documents

### FileState (probleme critique)

| Document | Variantes |
|----------|-----------|
| `spec-draft.md` | Ghost, Local, Modified, InConflict, Synced |
| `INTERFACES.md` | Ghost, Hydrated, Modified, Syncing, Error |
| `ipc-contract-design.md` | Ghost, Hydrated, Modified, Syncing, **Conflict**, Error |
| `vfs-engine-design.md` | Ghost, Hydrated, Modified, Syncing, **Conflict**, Error |
| `project-initialization-design.md` | Ghost, Hydrated, Modified, Syncing, **Synced**, Conflict, Error (7 variantes) |

**5 definitions differentes de FileState** a travers les documents. Le spec-draft utilise `Local` et `InConflict`, l'INTERFACES.md omet `Conflict`, et le design d'initialisation ajoute `Synced`. Il n'y a pas de source de verite claire.

### Contrat IPC

- `INTERFACES.md` definit 4 methodes : `file.status`, `file.hydrate`, `file.list`, `auth.token`
- `ipc-contract-design.md` definit 5 methodes : les 3 file.*, `events.subscribe`, `events.emit` — **mais pas `auth.token`**
- Le bridge JS dans `INTERFACES.md` expose `getToken()`, mais aucune methode IPC correspondante dans la spec IPC detaillee

### FileNode

- `INTERFACES.md` : `id: String`, `modified: String`
- `vfs-engine-design.md` : `id: Uuid`, `modified: OffsetDateTime`, + champ `parent_id`
- `ipc-contract-design.md` : `id: String`, `modified: String` (sans `parent_id`)

Le modele interne (VFS) et le modele IPC divergent, ce qui est potentiellement intentionnel, mais jamais documente.

---

## 3. Coherence des timelines

**Confusion entre 3 temporalites :**

| Document | Temporalite |
|----------|-------------|
| `PLAN.md` | Plan 6 semaines (developpement complet) |
| `PLAN_HACKATON.md` / `INTERFACES.md` / `STREAM_*.md` | Hackathon 48h |
| `project-initialization-design.md` / `project-initialization.md` | Semaine 1 (J1-J3) |

Le plan d'initialisation (J1-J3) et le hackathon (48h) couvrent des perimetres quasi identiques avec des decoupages differents. On ne sait pas si l'un remplace l'autre ou s'ils s'adressent a des contextes differents. Le `PLAN.md` 6 semaines ne reference pas les documents d'initialisation.

---

## 4. Qualite des specs detaillees (docs/superpowers/specs/)

**Points forts :**
- Les 4 design specs (CEF, VFS, IPC, Reconciliation) sont homogenes en structure : Overview -> Architecture -> Components -> Error Handling -> Testing -> Risks -> References
- Chaque spec inclut du code Rust/C++ concret, pas seulement de la prose
- Le trait `ReconciliationEngine` et le pattern Phase 1/Phase 2 sont bien articules
- Les diagrammes ASCII sont coherents entre documents

**Points faibles :**
- Toutes les specs sont en statut "Draft" sauf `project-initialization-design.md` (Approved). Le `spec.md` est "Final" mais pointe vers des specs Draft — incoherence de maturite.
- Le `cef-shell-design.md` a un checklist sur 5 jours alors que le hackathon est sur 2 jours et l'initialisation sur 3 jours
- Le `reconciliation-engine-design.md` mentionne `PouchDB` (crate `pouchdb-rs`) dans les dependances, qui n'existe probablement pas en Rust — confusion entre le concept CouchDB-style et l'implementation reelle

---

## 5. Coherence linguistique

Les documents melangent francais et anglais de facon inconsistante :
- `spec.md`, `ipc-contract-design.md`, `vfs-engine-design.md` : **anglais**
- `INTERFACES.md`, `STREAM_*.md`, `PLAN_HACKATON.md` : **francais**
- `spec-draft.md` : **mixte** (prose anglaise, rationale en francais)
- `project-initialization-design.md` : **mixte** (structure anglaise, contenu francais)

Ce n'est pas bloquant, mais c'est un signe de maturation progressive sans harmonisation.

---

## 6. ADR

**Points forts :**
- Bonne utilisation du format ADR avec contexte/decision/consequences
- L'ADR-0003 (architecture deux processus) est particulierement bien argumente

**Points faibles :**
- L'ADR-0002 est en statut "Proposed" alors que l'OIDC PKCE est deja mentionne comme decision prise partout ailleurs
- Certaines decisions documentees dans `spec-draft.md` (choix CEF vs Electron, CouchDB vs CRDT Phase 1/2) meriteraient leur propre ADR

---

## 7. Liens et navigation

- Les liens relatifs dans `spec.md` (`../../PLAN.md`, `../superpowers/specs/*.md`) sont corrects par rapport a la structure
- Le `cef-shell-design.md` reference `jsonrpsee C++ client` avec un lien vers le repo Rust jsonrpsee — c'est incorrect, jsonrpsee est une lib Rust
- Pas de document d'index global (type table des matieres) en dehors de `spec.md` qui ne liste pas tout

---

## Synthese

| Critere | Note | Commentaire |
|---------|------|-------------|
| **Structure** | 7/10 | Bonne hierarchie, mais 3 niveaux et documents racine mal integres |
| **Coherence des donnees** | 4/10 | FileState, FileNode et contrat IPC divergent entre documents |
| **Coherence temporelle** | 5/10 | 3 timelines (6 sem, 48h, 3 jours) non articulees entre elles |
| **Qualite des specs** | 8/10 | Specs detaillees bien structurees avec code concret |
| **ADR** | 7/10 | Bon format, mais incomplet (decisions majeures non couvertes) |
| **Navigation** | 6/10 | Liens corrects, mais pas de point d'entree unique |
| **Coherence linguistique** | 5/10 | Melange FR/EN sans convention |

**Verdict global : 6/10** — Les specs individuelles sont de bonne qualite, mais l'ensemble souffre d'un manque de coherence inter-documents. Le probleme prioritaire est la divergence de `FileState` qui est le type fondamental du systeme et qui a 5 definitions differentes. Le second probleme est la relation non clarifiee entre le hackathon 48h et le plan d'initialisation J1-J3.

---

## Recommandations prioritaires

1. **Etablir un document canonique unique pour les types partages** (FileState, FileNode, methodes IPC) et faire pointer tous les autres documents vers celui-ci plutot que de redefinir les types a chaque fois.
2. **Clarifier le statut de `spec-draft.md`** — archiver ou fusionner dans `spec.md`.
3. **Articuler les 3 timelines** — expliquer comment hackathon, initialisation J1-J3, et plan 6 semaines se relient.
4. **Ajouter un README.md** a la racine comme point d'entree.
5. **Choisir une langue** (FR ou EN) et harmoniser.
