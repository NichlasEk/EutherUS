# EutherUS / System Regis — handoff 2026-10-12

## Börja här

Projektet finns i `/run/media/nichlas/Kingston1TB/EutherUS`, på grenen `main`.
Det är ett eget Git-repo med remote https://github.com/NichlasEk/EutherUS.git.
Läs `AGENTS.md` och denna fil innan nästa ändring. Kontrollera sedan
`git status --short --branch` och aktuella rapporter; bygg inte om av gammal vana.

Senast fullständigt verifierade kodcommit är
`f36e337da4ff92514ff2ecc044764277e210af33`. Denna handoff är en efterföljande
ändring av dokumentationen. Testresultaten nedan gäller kodcommitten, inte en
ny körning efter dokumentationsändringen.

Nästa föreslagna milstolpe är **task-kontexter och en liten kooperativ
schemaläggare med skyddade stackar**. Grafik, input och en testuppgift kan då
bli verkliga kernel-uppgifter och senare ge tornet verklig aktivitet att visa.
Processmodell, programmodell och säkerhetsarkitektur behöver fortfarande
förankras i projektets premisser.

## Vision och vad som faktiskt kör

EutherUS är ett nytt Rust-operativsystem. Befintliga EutherBoot är dess
Rust-baserade UEFI-startpunkt; vi bygger inte en ny GRUB-ersättare här.
System Regis ska bli ett levande process- och resursträd kring ett roterande
ASCII-torn, med en kontextuell prompt och tydligt ägarskap över maskinen.

Det finns två separata körbara implementationer:

- **Native Linux-preview:** interaktivt torn, processgrenar, prompt, inspektör
  och kamera. Processer och minnesdata är uttryckligen simulerade.
- **Riktig QEMU-kernel:** samma delade torngeometri renderas till framebuffer
  efter ExitBootServices. Space pausar/återupptar animationen. Visade
  pool-/arenastorlekar kommer från kernelns allocator. Processgrenar, prompt,
  schemaläggare och userspace finns ännu inte i gästen.

QEMU-bilden är alltså verklig kernelgrafik, inte en GIF. Alla accepterade
kerneltester är körda i TCG; fysisk hårdvara och KVM är separata, overifierade steg.

## Implementerat

| Del | Nuvarande beteende |
| --- | --- |
| EutherLab | Python-standardbibliotek, CLI och versionerat JSON-lines-API; ingen LLM behövs för testorkestrering. |
| VM-isolering | Egen qcow2-overlay, OVMF-vars, loggar, QMP/GDB Unix-sockets och verifierad PID-identitet per instans. Nätverk avstängt. |
| Bygge | Snapshot av befintliga EutherBoot, låsta Rust-beroenden och manifest med commits, dirty-status, innehållshashar, toolchain och firmware-identitet. |
| Boot | OVMF → EutherBoot → UEFI Rust-entry → ExitBootServices → no_std-kernel → strukturerade COM1-händelser. |
| CPU | Egen GDT/TSS/IDT, kernelstack och emergency-stack, dubbel-fel via IST, PIT/PIC-timer och HLT-väntan. |
| Fysiskt minne | Testad 4 KiB-frameallocator, explicita reservationer och konservativ pool av konventionellt minne under 1 GiB. |
| Paging | Egen kopia av sidtabellshierarkin, verifierat CR3-byte, fyra omappade stackguards och uppdelning av stora pages där det behövs. |
| Grafik | Delad no_std-geometri, RGB-framebuffer, Space-input och 6 MiB arena från kernelns allocator. |

`graphics` inkluderar paging, memory och interrupts. `paging` inkluderar memory.
Normalvarianten är fortfarande den lilla seriella boot-baselinen; den har inte
alla dessa experimentella lager. Intern BootInfo är v3, 112 byte, för
memory/paging/graphics; normal/CPU-varianter använder v1. Se respektive dokument
för kontrakt och feature-gränser.

Kernelns och emergency-stackens användbara storlek är vardera 128 KiB, med
4 KiB guards på båda sidor när paging är aktivt. Sidtabellerna bevarar i övrigt
firmwarets leaf-mappningar och rättigheter. Gamla firmwaretabeller och
icke-konventionellt minne förblir reserverade. Ingen firmware-reclamation,
global heap, SMP, userspace eller generell dynamisk mapping-API finns ännu.

## Verifierad evidens

Sammanställning: `artifacts/paging-milestone.json`, från ren kodcommit
`f36e337da4ff92514ff2ecc044764277e210af33`.

| Svit | Resultat | Lokal rapport |
| --- | --- | --- |
| Paging | PASS: CR3, fyra direkta guard-fel, stackoverflow via emergency IST, parallella VM och reset | `artifacts/reports/paging-20261012-090955-495e71.json` |
| Grafik | PASS: riktig rendering/input i QEMU och isolerade instanser | `artifacts/regis/qemu/20261012-090957-1b6b0e/report.json` |
| Minne | PASS: reservationer, RAM-mönster, exhaustion, invalid-map/OOM och livscykel | `artifacts/reports/memory-20261012-091034-749eef.json` |
| CPU | PASS: timer, register, reset och riktiga UD/GP/PF/DF-fel samt maskerad timer | `artifacts/reports/interrupts-20261012-091114-16e8fd.json` |
| Ursprungligt labb | PASS: normal boot, JSON API, parallell isolering, panic, timeout och crash | `artifacts/reports/20261012-091217-c421b9.json` |

Dessutom passerade 7 `regis-memory`-tester och 9 Python-controller-tester.
Vid slutkontrollen fanns inga aktiva controllerägda VM-instansprocesser.
Det säger inget om huruvida en separat native-preview fortfarande är öppen.

Senaste riktiga QEMU-bild: `artifacts/regis/qemu-tower.png`.
Rapportpekare finns också i `artifacts/paging-latest.json`,
`artifacts/memory-latest.json`, `artifacts/interrupts-latest.json` och
`artifacts/regis/qemu-latest.json`.

Known-good är fortfarande den ursprungliga seriella labbversionen
`38e3cbed592c90fc432d9b2a7b0b7f986a7efa63`, via `.lab/known-good.json`.
Grafik/paging-experimenten har inte promoverats. Behåll dess refererade
byggartefakter och rapport om labbet flyttas.

## Återuppta och reproducera

Värdberoenden: Python 3.11+, Git, QEMU system x86 och qemu-img, OVMF,
dosfstools, mtools, GDB och curl. Native-preview använder X11; dess automatiska
UI-tester körs separat i Xvfb. Se [README](README.md) för paket och firmwarevägar.

EutherBoot-källan förväntas i `../EutherBoot`; sätt `EUTHERBOOT_SOURCE` för annan
plats. Bygget kopierar källan och ändrar inte sibling-checkouten. Dess eventuella
lokala ändringar registreras i manifestet och arkiveras med byggartefakten.

```sh
./scripts/setup.sh
export CARGO_HOME="$PWD/.tools/cargo"
export RUSTUP_HOME="$PWD/.tools/rustup"
export PATH="$CARGO_HOME/bin:$PATH"

cargo test --locked -p regis-memory -p regis-model -p regis-preview -p regis-scene
python3 -m unittest discover -s tests -v
python3 scripts/test-paging-qemu.py --build
./eutherlab build --variant graphics
python3 scripts/test-regis-qemu.py
python3 scripts/test-memory-qemu.py --build
python3 scripts/test-interrupts-qemu.py --build
python3 scripts/validate.py
```

Toolchain är projektlokal `nightly-2026-10-01`, target
`x86_64-unknown-uefi`. Setup ändrar inte shellprofilen. Normala VM-inställningar
är Q35, TCG, 256 MiB RAM, en CPU och inget nätverk.

För en enskild grafikinstans efter bygge:

```sh
./eutherlab boot --vm regis --variant graphics
./eutherlab status --vm regis
./eutherlab logs --vm regis
./eutherlab screenshot --vm regis
./eutherlab debug --vm regis --registers
./eutherlab stop --vm regis
./eutherlab clean --vm regis
```

QEMU kör headless; screenshot hämtar den verkliga skärmen via QMP. Boot-kommandot
väntar på QMP, inte på kernelns boot-success: kontrollera status/loggar eller
använd testsuiten. Stop bevarar loggar; clean vägrar ta bort en levande VM.
`reset` är varm reset; `reset --fresh` återställer disk och firmware-vars från
samma immutabla bygge. Använd aldrig breda processdödningar.

För den interaktiva Linux-formstudien: `./scripts/regis-preview.sh`.
F3 växlar rörelse, F4 släpper kameralås, F2 visar inspektör, F10 sparar bild,
Ctrl+Q avslutar. Kör inte automatiska UI-tester mot användarens öppna fönster.

## Kodkarta och nästa arbetssteg

| Fil/dokument | Ansvar |
| --- | --- |
| `lab/controller.py`, `lab/validation.py` | VM-livscykel, API, byggmanifest och basacceptans |
| `crates/kernel/src/interrupts.rs` | CPU-tabeller, stackar, undantag och timer |
| `crates/kernel/src/memory.rs`, `crates/regis-memory/` | Minnessäkerhetskontroller, frames och rena allocator/split-tester |
| `crates/kernel/src/paging.rs` | Bygga/auditera egna tabeller, guards och CR3-byte |
| `crates/kernel/src/graphics.rs`, `crates/regis-scene/` | Framebufferrendering och delad torngeometri |
| `crates/regis-model/`, `crates/regis-preview/` | Simulerad processmodell och native-formstudie |
| [Arkitektur](docs/ARCHITECTURE.md), [API](docs/API.md) | Labbkontrakt och maskinläsbart gränssnitt |
| [CPU](docs/CPU-BOOTSTRAP.md), [Minne](docs/MEMORY-BOOTSTRAP.md), [Paging](docs/PAGING-BOOTSTRAP.md) | Nuvarande kernelmekanismer och begränsningar |
| [Grafik](docs/GRAPHICS-BOOTSTRAP.md), [System Regis](docs/SYSTEM-REGIS.md) | Gästgrafik respektive native-interaktion |

Nästa implementation bör börja med en liten dokumenterad task-context och
kooperativ yield, egna skyddade task-stackar samt deterministiska serialtester
som bevisar att två uppgifter gör framsteg och behåller register/stackdata.
Behåll timer/undantagsvägarna separata från task-byte tills det är verifierat.
Koppla sedan riktig task-telemetri till tornet. Full processisolering och
firmware-reclamation är senare arkitekturbeslut.

## Git, artefakter och säkerhetsgräns

Push omfattar källkod, tester och dokumentation. `.tools/`, `.lab/`, `target/`,
`artifacts/` och `worktrees/` är ignorerade och följer inte med Git. En ny clone
måste installera toolchain, ha EutherBoot-källan tillgänglig och generera egna
byggbilder/rapporter; den får inte den lokala known-good-pekaren automatiskt.

Ändra inte värdens bootposter, installerade EutherBoot, fysiska diskar eller
firmwareinställningar. Fysisk hårdvara kräver separat uttrycklig instruktion.
Labbet använder disponibla virtuella resurser. VM-isoleringen är inte en
säkerhetssandbox för en kodagent med obegränsad shellåtkomst.
