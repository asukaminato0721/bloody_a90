# P85 driver comparison

The reviewed [P85 project](https://gitlab.com/C0rn3j/a4tech_bloody_p85_driver)
is at commit `912a769664801aff1c4f26f4c815b35fac4ee9ad`.
The remote HEAD matched this commit during the review.
The review reads source files. It does not run either P85 application.

## Decision

Keep Piper and the A90 libratbag driver. Use the P85 project as a protocol
reference. Changing the application does not resolve the missing A90
sensor conversion, persistent configuration or button/macro formats.

| Area | P85 source | A90 evidence | Result |
| --- | --- | --- | --- |
| Transport | HID feature 7 over libusb, 64 or 72 bytes. Interface 2 is fixed in transfers. | The A90 feature report has 64 bytes. The current driver selects the vendor HID interface. | Shared framing is useful. Keep the current hidraw transport. |
| Identification | `getDeviceInfo` reads `1f` and maps model `04fc` to A90 firmware. | Observed raw model word `04fd`, with model bits `04fc`. | Useful independent agreement. |
| Brightness | Command `11`, write bit `80`, values 0–3. | All four levels passed earlier hardware readback. | The existing A90 implementation already covers this function. |
| Runtime DPI | `SetSensitivity` puts a calculated DPI code in both scale bytes. It sets the sensor byte to zero. | Factory scales are 64. Separate sensor codes are `17`, `1b`, `24`, `2e`, `91`. | The P85 encoder does not fit this A90 sensor. |
| Persistent DPI | `saveSensToMemory` uses command `01` and fixed word address `2a00`. | A90 memory information reports writable sensitivity address `2400`. | Do not reuse the P85 write sequence. |
| Initial settings | The Qt UI supplies fixed defaults and marks settings extraction as TODO. | A90 raw runtime values and factory tables are available from previous reads. | A90 needs its own settings parser. |
| Buttons and macros | No configuration API for these functions exists in `Mouse.h`. | These formats remain under analysis. | This project does not provide the missing implementation. |

Relevant files in the reviewed checkout:

- `Mouse.cpp:993`: identification.
- `Mouse.cpp:1058`: runtime DPI encoding.
- `Mouse.cpp:1327`: persistent sensitivity writes.
- `qt/mainwindow.cpp:83`: fixed initial settings.
- `README.md`: project status and protocol observations.
- `mice.yaml`: `A90cir_P3332A_4FC` model entry.

The P85 notes helped locate the flash read interface.
The A90 firmware and official application provide the evidence for
A90-specific fields. A matching command number alone does not establish
that its parameters have the same meaning on both models.
