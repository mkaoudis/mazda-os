# References

Primary sources and useful community work for Mazda Connect Gen 6 / 6.5 research. Prefer Mazda service material and direct technical research for factual claims; use community projects as implementation references.

## Firmware and access map

| Firmware | Relevance |
| --- | --- |
| v55–v58 | Legacy tweak-friendly era; MZD-AIO supports direct USB-installed tweaks. |
| v59 < 59.00.502 | Still compatible with the older AIO access model. |
| v59.00.502+ | Important lock-down boundary; later AIO workflows generally require serial access or previously installed recovery/autorun support. |
| v70.00.000 / .021+ | CarPlay / Android Auto era. |
| v70.00.335+ | Additional lock-down; legacy autorun-based access is removed and serial-assisted methods become more important. |
| v74.00.324A | Main production target for this project. Mature Gen-6 firmware; ZDI demonstrated root code execution on this exact release. |
| v74.00.331 | Community-reported on some replacement CMUs; do not target unless encountered on hardware. |

## Primary technical references

- [Zero Day Initiative — Multiple Vulnerabilities in the Mazda IVI System](https://www.zerodayinitiative.com/blog/2024/11/7/multiple-vulnerabilities-in-the-mazda-in-vehicle-infotainment-ivi-system) — teardown and exploitation of firmware 74.00.324A; documents the i.MX6 application processor, Linux 3.0.35, update format, VIP separation, USB attack surface, and lack of a strong application-processor root of trust.
- [Mazda CMU Documentation](https://github.com/silverchris/mazda-cmu-documentation) — community hardware/software documentation covering the CMU, firmware/update format, boot process, VIP, D-Bus, kernel configuration, debug interfaces, and related internals.
- [Mazda Service Alert SA-008/24](https://static.nhtsa.gov/odi/tsbs/2024/MC-11006998-0001.pdf) — official Gen-6 service guidance covering affected Mazda models and CMU software troubleshooting/update expectations.
- [Mazda TSB 09-018/22](https://static.nhtsa.gov/odi/tsbs/2022/MC-10226834-0001.pdf) — official v74-era software-fix documentation referencing 74.00.324 or later.
- [Mazda Service Alert SA-065/17](https://static.nhtsa.gov/odi/tsbs/2017/MC-10118479-9999.pdf) — useful historical reference for the 59.00.502-era Bluetooth/software transition.
- [Mazda firmware changelogs](https://github.com/drone540/mazda-firmware-changelogs) — community-maintained version history across early Gen-6 through v70.
- [Miatafy Gen-6 firmware/version reference](https://miatafy.com/firmware/versions/) — modern consolidation of v70/v74 versions, package metadata, regional differences, and the v74.00.331 caveat.

## Rooting and tweak ecosystem

- [Mazda AIO Tweaks](https://mazdatweaks.com/) — canonical legacy tweak project; especially useful for firmware compatibility boundaries and examples of custom UI/apps running on the stock CMU.
- [mzd-evo/mzd-connect-1-root](https://github.com/mzd-evo/mzd-connect-1-root) — modern root/access research used by later Gen-6 projects.
- [shunceyb/mzd74-tweaks-no-touch](https://github.com/shunceyb/mzd74-tweaks-no-touch) — v74-specific tweak/root work demonstrating that later firmware remains modifiable.
- [Miatafy/TouchTune](https://github.com/Miatafy/TouchTune) — current v74.00.324A USB-install work; useful evidence for a no-disassembly USB entry path on the main target firmware. The path still executes code as root and TouchTune itself makes persistent changes.

## Native application and UI development

- [Mazda Custom Application SDK](https://github.com/flyandi/mazda-custom-application-sdk) — custom applications, Commander input, lifecycle, and vehicle-data abstractions on the stock JCI application stack.
- [CASDK simulator](https://github.com/flyandi/mazda-custom-application-sdk-simulator) — historical desktop simulator for Mazda custom applications.
- [Updated CASDK simulator](https://github.com/Romano-Garmez/mazda-custom-application-sdk-simulator) — newer simulator fork useful as a behavioral reference for desktop development.
- [jmgao/mazda-connector](https://github.com/jmgao/mazda-connector) — native ARM binaries, startup integration, D-Bus interaction, and use of Mazda's native Bluetooth stack.
- [rpendleton/mazda-toolchain](https://github.com/rpendleton/mazda-toolchain) — cross-compilation tooling for binaries compatible with the CMU's older ARM/Linux environment.
- [Mazda CASDK Apps](https://github.com/romangarms/Mazda-CASDK-Apps) — examples of real custom applications and available data/service integrations.

## Working assumptions for mazda-os

- Treat **74.00.324A** as the canonical production firmware target unless bench hardware proves otherwise.
- Firmware lock-down primarily changes how privileged execution is obtained; once access is available, the CMU remains a conventional embedded Linux application platform.
- Keep all project code on the application-processor side. Do not depend on VIP flashing, raw vehicle-bus access, or vehicle-control capabilities.
- Validate important community claims against the bench CMU before making them architectural dependencies.
