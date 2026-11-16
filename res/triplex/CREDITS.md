# Credits

Assets used by the triplex acies demo (`crates/mira_ui/examples/triplex`).

| What | Where | Licence |
|---|---|---|
| The men in `sprites/`: the Rocketbox figure *m013* as the Paris demo rigged it (`res/paris/models/citizen.glb`), dressed as Roman legionaries, a legate, Caesar and Gallic warriors and rendered as sprite sheets by `crates/mira_ui/examples/triplex/tools/romans.py` | [Microsoft Rocketbox](https://github.com/microsoft/Microsoft-Rocketbox) | MIT, `LICENSE_MIT_Microsoft_Rocketbox.txt` |
| The horses under the legate and Caesar: the Paris demo's bay and chestnut (the chestnut repainted grey) | see `res/paris/CREDITS.md` | CC0 |

Made for the demo: the kit (Montefortino and Attic helmets, scuta, pila, swords, mail, cuirasses,
pteruges, cloaks, saddlery, Gallic shields), the painted tunics, mail and trousers, and the grass.

`sprites/sprites.json` says how the sheets are laid out: the camera's angle below the horizon
(35 degrees, orthographic), metres to a pixel, and for each kind the cell size, where the feet
are and the order of the frames. Rows are directions: the first faces the viewer, and each
after it turns the man a further 45 degrees to his left.
