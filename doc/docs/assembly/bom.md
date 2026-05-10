# Bill of Materials

These are the things that you need to get from stores:

| Item                                                  | Type            | Qty  | What I Bought                                                |
| ----------------------------------------------------- | --------------- | ---- | ------------------------------------------------------------ |
| Bourns PSL60                                          | Motorized fader | 1    | [PSL60-1082A-103B1](https://es.farnell.com/bourns/psl60-1082a-103b1/pot-desliz-10k-20-0-5w-pasante/dp/2838883) |
| Raspberry Pi Pico 2                                   | Microcontroller | 1    | [ASIN: B0DCKH85WR](https://amzn.eu/d/0idjIjIz)               |
| DRV8833 (on breakout board)                           | Circuit         | 1    | [ASIN: B0DHRFRG6X](https://amzn.eu/d/0flBWfpK)               |
| 2-Pin JST-XH Vertical Header (2.54mm)                 | Connector       | 1    | [ASIN: B0BZHR5NCR](https://amzn.eu/d/08iP7cfn)               |
| 2-Pin JST-XH Female Housing (2.54mm)                  | Connector       | 1    | [ASIN: B0BZHR5NCR](https://amzn.eu/d/08iP7cfn)               |
| 4-Pin JST-XH Vertical Header (2.54mm)                 | Connector       | 1    | [ASIN: B0BZHR5NCR](https://amzn.eu/d/08iP7cfn)               |
| 4-Pin JST-XH Female Housing (2.54mm)                  | Connector       | 1    | [ASIN: B0BZHR5NCR](https://amzn.eu/d/08iP7cfn)               |
| 4-Pin JST-PH Cables with Male Plug Connector (2.00mm) | Cable           | 1    | [ASIN: B0C9C9VF4T](https://amzn.eu/d/00JlXiUl)               |
| M3×6 hex socket head cap screw                        | Screw           | 8    | [ASIN: B0DQ19XG23](https://amzn.eu/d/08OMiqco)               |
| M3×8 hex socket countersunk head screw                | Screw           | 4    | [ASIN: B0DPWZ53X4](https://amzn.eu/d/03UlMjSr)               |
| M3×4.0×5.0 heat-set insert                            | Heat-set insert | 2    | [ASIN: B0B2DMWFT2](https://amzn.eu/d/09NEdmbo)               |
| M3×4.8×5.0 heat-set insert                            | Heat-set insert | 4    | [ASIN: B0B2DMWFT2](https://amzn.eu/d/09NEdmbo)               |
| M3×5.7×5.0 heat-set insert                            | Heat-set insert | 4    | [ASIN: B0B2DMWFT2](https://amzn.eu/d/09NEdmbo)               |

!!! note
    The "what I bought" links are not affiliated, and as a matter of fact most of them are not buyable anymore. But at least this way you can see exactly what I'm referring to.

## Operating Margin

Overall, I understand that you might not find where you are exactly what is listed above, depending on the country in which you are.

- **The fader** — As long as you have a fader with a 2-wired motor and the "1 2 3" pins, you should be good. We're not using the "T" pin, so you don't have to wire it in the connector if you don't have it. The software makes as few assumptions as possible, so any fader should be compatible. The design [in Onshape](https://cad.onshape.com/documents/e0a1011ad933c9838ad313ca/w/9072f4ad6800d0c6b8320443/e/6ad28f5df8e4bbfb02ddf18f?renderMode=0&uiState=69ffd1ceabeaa0112793777b) is also configured to be easily modified to fit a differently-sized fader.
- **Raspberry Pi Pico 2** — We don't do anything crazy, any Raspberry Pi Pico should do the job. The only thing is, it's totally untested. And even less tested with the W version, which has Wifi. Although there are no reasons to think it would fail.
- **DRV8833** — My understanding is that the breakout board is not standard for this component. So obviously you'll either have to find the same one or to re-wire the PCB.
- **JST connectors** — The PSL60 uses 2mm pins and the board 2.54mm pins. I'm counting on the cables to do the "translation" (see below). Change the size of the headers and you'll need to update the PCB a bit.
- **Screws** and **heat-set inserts** — Most places allow for longer screws or inserts without any major issue. Even a shorter insert is hardly ever going to be a problem, as the mechanical properties required for this assembly are very achievable. The main thing to be mindful of is to have sufficiently long screws, otherwise they risk not screwing anything. Also the screw heads (countersunk vs head cap) have their importance but then again it *would* hold even with the wrong head, if you're not afraid of ugly.
