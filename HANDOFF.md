# EutherUS / System Regis — handoff 2026-10-15

## Börja här

Projektet finns i `/run/media/nichlas/Kingston1TB/EutherUS`, på grenen `main`.
Remote är `https://github.com/NichlasEk/EutherUS.git`.
Läs `AGENTS.md`, denna fil och `docs/ARCHITECTURE.md` före nästa ändring.
Kontrollera aktuell arbetskopia och rapporter; bygg inte om av gammal vana.

**Ny milstolpe: en läsbar kontextuell kernelprompt med help, tasks, memory och inspect.**
Implementation, tester och dokumentation versionshanteras tillsammans i
milstolpecommitten efter `62912e9`. Genererade byggbilder och rapporter är lokala.
Senaste tidigare verifierade kodcommit är `f36e337`.
Full regression för den nya arbetskopian finns i `artifacts/prompt-milestone.json`.
Byggmanifesten anger dirty-status och exakt innehållshash; hänvisa inte till
`62912e9` som om den committen innehöll task-implementationen.
Denna handoff uppdaterades efter regressionen; ingen körbar kod ändrades då.

## Vision och vad som faktiskt kör

EutherUS är ett Rust-operativsystem med befintliga EutherBoot som UEFI-startpunkt.
System Regis ska bli ett levande process- och resursträd kring ett roterande
ASCII-torn, med kontextuell prompt och tydligt ägarskap över maskinen.

- **Native Linux-preview:** interaktivt torn, processgrenar, prompt, inspektör
  och kamera. Processer och minnesdata är fortfarande simulerade.
- **QEMU graphics:** renderer och PS/2-input kör som två långlivade tasks på
  egna skyddade stackar. Tornets grenar visar verkliga dispatches, bildrutor,
  inputpollningar och yields. Space pausar rotationen medan tasks/räknare
  fortsätter. Framebufferarenan kommer från kernelns allocator. En läsande
  inspektör visar den valda uppgiftens tillstånd, väntan, stackgränser och räknare.
  Tab/pilar väljer gren, F2 visar/döljer panelen och Esc stänger den.
- **QEMU tasks:** separat seriell variant med två verkliga kerneluppgifter.
  De kör kooperativt på varsin skyddad stack, yieldar åtta respektive fem gånger,
  avslutas och lämnar tillbaka kontrollen till kernelns interruptdrivna idle.

TCG är acceptansbaslinjen. KVM och fysisk hårdvara är inte verifierade.
Processmodell, programmodell och övergripande säkerhetsarkitektur är fortfarande
öppna; två kerneluppgifter innebär inte userspace eller processisolering.

## Implementerat

| Del | Beteende |
| --- | --- |
| EutherLab | Python-standardbibliotek, CLI/JSON-lines-API, isolerade overlays, OVMF-vars, QMP/GDB och verifierad PID-identitet. |
| Boot | OVMF → snapshot av EutherBoot → UEFI Rust-entry → ExitBootServices → no_std-kernel → strukturerade COM1-händelser. |
| CPU | Egen GDT/TSS/IDT, kernelstack, double-fault IST-stack, PIT/PIC och HLT. |
| Minne | Testad 4 KiB-frameallocator med explicita reservationer och konservativ pool under 1 GiB. |
| Paging | Egna kopierade sidtabeller, CR3-byte, guards och splitting av stora pages. |
| Grafik/input | Två separata tasks; delad torngeometri, framebuffer, Space-input och 6 MiB allocatorarena. |
| Tasks | Två statiska 128 KiB-stackar, fyra ytterligare guards, Ready/Running/Sleeping/Finished och kooperativ round-robin. |
| Inspektör | Kopierade, läsande scheduler-snapshots; markerad gren, tangentbordsval och live-panel. |

Normalvarianten är fortfarande den lilla seriella baslinjen. `graphics` och
`tasks` använder samma scheduler och inkluderar paging → memory → interrupts.
Den seriella `tasks`-varianten behåller sina två ändliga testuppgifter.
BootInfo är v3 (112 byte) för memory/paging/graphics/tasks, v1 för normal/CPU.

Task-byte använder en explicit SysV64-anropsgräns inne i Win64/UEFI-kerneln.
RSP/returadress, callee-saved GPR, RFLAGS, MXCSR och x87-kontrollord bevaras.
Det är en pausad funktionskontext, inte en full interrupt-/preemptionskontext.
IRQ-handlern schemalägger aldrig. Statisk stacklagring ligger reserverad i
kernelbilden; den allokeras/frigörs inte dynamiskt.
Grafik sover fem tick efter varje bild, input ett tick efter högst 32 lästa bytes.
Sleep yieldar till schedulern; när båda sover väntar schedulern med HLT på sin
stack. Timern väcker genom att räkna tick men gör aldrig ett task-byte.

En oväntad retur från grafik/input är fatal (task_finished följt av panic).
Input-guard-fel ger #PF på dess egen stack. Ingen task-restart/felåterhämtning
finns. Grafiken läser kopior av räknarna och inputstatus; input rör inte arenan.
Inspektören förändrar inte task-tillstånd. Den visar grafiken som running när
grafiken ritar, och input normalt som sleeping med timerdeadline. En förfallen
deadline betyder inte ready förrän schedulern gör övergången. Stackstorleken är
reserverad kapacitet, inte uppmätt stackförbrukning. F1 öppnar den läsande
prompten; Esc/F1 stänger med bevarat utkast. Med fokus skriver Space mellanslag,
och Tab/pilar/F2 ändrar inte gren. Help, tasks, memory och inspect läser data;
inspect utan argument använder vald gren. Input ser sig själv running när
kommandot körs. Minnesvärdena fångas efter arenans allokering; inga senare
allokeringar sker. Se `docs/PROMPT.md`, `docs/INSPECTOR.md`, `docs/TASKS-BOOTSTRAP.md` och
`docs/GRAPHICS-BOOTSTRAP.md` före utökning.

Firmwaretabeller och icke-konventionellt minne är fortfarande reserverade.
Ingen firmware-reclamation, global heap, SMP, userspace eller generell dynamisk
mapping-/task-spawn-API finns. En task som inte yieldar kan svälta den andra.

## Verifierad evidens

Sammanställning: `artifacts/prompt-milestone.json`.
Körloggar och testad källidentitet: `artifacts/prompt-regression/summary.json`.

| Svit | Resultat | Lokal rapport |
| --- | --- | --- |
| tasks | PASS | `artifacts/reports/tasks-20261015-183048-8cac29.json` |
| paging | PASS | `artifacts/reports/paging-20261015-183127-7bf481.json` |
| graphics | PASS | `artifacts/regis/qemu/20261015-183128-44db73/report.json` |
| inspector | PASS | `artifacts/regis/inspector/20261015-183200-ff2c95/report.json` |
| prompt | PASS | `artifacts/regis/prompt/46030bff8506/report.json` |
| graphics-faults | PASS | `artifacts/reports/graphics-faults-20261015-183349-30d7ac.json` |
| memory | PASS | `artifacts/reports/memory-20261015-183410-abc9ab.json` |
| cpu | PASS | `artifacts/reports/interrupts-20261015-183449-af91fe.json` |
| baseline | PASS | `artifacts/reports/20261015-183551-f37a73.json` |

Task-sviten bevisar exakt progressordning, registersentineler i assembly,
volatile stackmönster, separata FP-kontrollvärden och bevarad interruptflagga.
Båda uppgifterna gör framsteg före injicerade fel. Fyra direkta guardskrivningar
ger #PF med exakt CR2/error code; ett framtvingat task-stacköverflöde ger #DF
på emergency IST. Tester inkluderar två VM, varm/full reset och isolerat stopp.
Grafiksviten bevisar riktiga task-stackar, sleep/wakeup, scheduler-HLT, live
räknare även under paus, inputisolering och varm/full reset. Två extra
QEMU-felvarianter provar oväntad input-retur och ett faktiskt input-guard-fel
**efter boot_success och verklig aktivitet**, följt av halt och korrekt fatal
status från controllern. Inspektörsviten provar verklig QMP-tangentbordsinput,
Tab/pilar, F2/Esc, hållna/upprepade tangenter, ignorerade tangenter, pixeländringar,
data mot task-/frame-händelser, uppdatering under rotationspaus, två-VM-isolering
och varm/full reset av val/panel. Promptsviten provar fokus och bevarat utkast,
redigering, Shift, verkliga task-/minnesdata, felaktiga argument, radgräns,
upprepade tangenter, kommandots eventordning, panelpixlar, VM-isolering och reset.
Dessutom passerade 15 Rust-tester och 9 Python-tester.

Senaste granskade riktiga QEMU-bild: `artifacts/regis/qemu-prompt.png`.
Tidigare rapporter finns kvar via `artifacts/graphics-tasks-milestone.json`
och `artifacts/tasks-milestone.json`.

Known-good är fortfarande den ursprungliga seriella labbversionen
`38e3cbed592c90fc432d9b2a7b0b7f986a7efa63`, via `.lab/known-good.json`.
Ingen experimentvariant har promoverats. Bevara dess refererade artefakter.
Vid slutkontrollen fanns inga aktiva controllerägda VM-processer.

## Återuppta och reproducera

Toolchain: projektlokal `nightly-2026-10-01`, target `x86_64-unknown-uefi`.
Värdberoenden och setup beskrivs i README. EutherBoot förväntas i `../EutherBoot`
eller via `EUTHERBOOT_SOURCE`; bygget ändrar inte sibling-checkouten.
Normala VM: Q35, TCG, 256 MiB RAM, en CPU, inget nätverk.

```sh
export CARGO_HOME="$PWD/.tools/cargo"
export RUSTUP_HOME="$PWD/.tools/rustup"
export PATH="$CARGO_HOME/bin:$PATH"
cargo test --locked -p regis-memory -p regis-model -p regis-preview -p regis-scene
python3 scripts/test-tasks-qemu.py --build
python3 scripts/test-paging-qemu.py --build
./eutherlab build --variant graphics
python3 scripts/test-regis-qemu.py
python3 scripts/test-inspector-qemu.py
python3 scripts/test-prompt-qemu.py
python3 scripts/test-graphics-faults-qemu.py --build
python3 scripts/test-memory-qemu.py --build
python3 scripts/test-interrupts-qemu.py --build
python3 scripts/validate.py
```

En enskild task-instans, med redan byggd variant:

```sh
./eutherlab boot --vm tasks-demo --variant tasks
./eutherlab status --vm tasks-demo
./eutherlab logs --vm tasks-demo
./eutherlab stop --vm tasks-demo
./eutherlab clean --vm tasks-demo
```

`boot` väntar på QMP, inte boot-success. `status.task_events` visar task-händelser.
QEMU är headless. För tornet: använd `--variant graphics` och `screenshot`.
Stop bevarar loggar; clean vägrar levande VM. `reset` är varm reset;
`reset --fresh` återställer disk/firmware-vars från samma immutabla bygge.
Döda aldrig QEMU-processer via breda processmatchningar.

Native-formstudie: `./scripts/regis-preview.sh`. F3 rörelse, F4 kameralås,
F2 inspektör, F10 bild, Ctrl+Q avslut. Automatiska UI-tester körs separat i Xvfb.

## Nästa arbetssteg och kodkarta

Prompten och inspektören är implementerade. Prompten har 64 teckens rad,
åtta utdatarader, US ASCII-inmatning och Backspace i slutet. Ingen historik-
navigering, Unicode eller programstart finns. Nästa större steg är ännu inte
beslutat. Grafiken har två fasta task-grenar; den generella
processmodellen från native-previewen har inte flyttats in. Diskutera större
program-/resursbeslut med användaren när de blir aktuella.
Behåll timer/undantag separata från task-byte. Dynamisk task-livscykel,
processisolering och firmware-reclamation är senare arkitekturbeslut.

| Fil | Ansvar |
| --- | --- |
| `crates/kernel/src/prompt.rs`, `scripts/test-prompt-qemu.py`, `docs/PROMPT.md` | Läsande konsol, fokus, begränsningar och faktisk QEMU-acceptans. |
| `crates/kernel/src/tasks.rs` | Kontextbyte, stackar, scheduler och task-prober. |
| `scripts/test-tasks-qemu.py` | QEMU-acceptans för task-progress, fel och livscykel. |
| `scripts/test-regis-qemu.py`, `scripts/test-graphics-faults-qemu.py` | Live grafik/input, telemetri, pixlar, reset och fatala task-fel. |
| `scripts/test-inspector-qemu.py`, `docs/INSPECTOR.md` | Tangentbordsval, läsande task-data, pixeltest, isolering och kontroller. |
| `crates/kernel/src/interrupts.rs` | CPU-tabeller, kernel/emergency-stackar, timer/undantag. |
| `crates/kernel/src/paging.rs` | Sidtabeller, CR3 och samtliga guards. |
| `crates/kernel/src/memory.rs`, `crates/regis-memory/` | Frames, reservationer och allocator/split-tester. |
| `crates/kernel/src/graphics.rs`, `crates/kernel/src/input.rs`, `crates/regis-scene/` | Grafik/input-tasks, framebuffer, telemetri och delad geometri. |
| `crates/regis-model/`, `crates/regis-preview/` | Simulerad modell och native-formstudie. |
| `lab/controller.py`, `lab/validation.py` | VM-livscykel, byggmanifest, API och basacceptans. |

## Git, artefakter och säkerhetsgräns

`.tools/`, `.lab/`, `target/`, `artifacts/` och `worktrees/` är ignorerade.
Källkod/tester/dokumentation får versionshanteras; genererade firmwarebilder,
diskar och loggar ska inte checkas in. En ny clone måste generera egna artefakter.

Ändra inte värdens bootposter, installerade EutherBoot, fysiska diskar eller
firmwareinställningar. Fysiska tester kräver separat uttrycklig instruktion.
Labbet äger endast sina disponibla virtuella resurser.
