# PCB & Soldering

My understanding is that there are many ways to do this, however here is how I did it.

Within the source code, under the [`hardware/kicad`](https://github.com/Xowap/dhd/tree/develop/hardware/kicad) folder, you can find the schematics for the PCB, to be opened in the free software [KiCad](https://www.kicad.org/).

## Building the Board

!!! warning "Read first"
    Before building the board, read the instructions of the "2-pins header — The motor" below, because it might impact your process.

Then, up to you. If you know how to do this chemically or with a laser or whatever the cool kids do, knock yourself out. In my case, I decided to order it from PCBWay, because it's much easier. Here are the steps that I followed:

1. Open the PCB in KiCad (the project is in [`hardware/kicad/`](https://github.com/Xowap/dhd/tree/develop/hardware/kicad))

2. Export Gerber files:

    - Go to **File → Fabrication Outputs → Gerbers (.gbr)...**
    - Output directory: `gerbers/` (or any folder you like)
    - Make sure all layers are checked (F.Cu, B.Cu, F.Mask, B.Mask, F.Silkscreen, B.Silkscreen, Edge.Cuts, F.Paste, B.Paste)
    - Click **Plot**

3. Export drill files:

    - Still in the Gerber dialog, click **Generate Drill Files...**
    - Format: **Excellon**
    - Drill origin: **Absolute**
    - Click **Generate Drill File**

4. Put all those files (`.gbr` + `.drl`) at the root of a `.zip`

5. Go to PCBWay and use the [PCB Quick Order](https://www.pcbway.com/QuickOrderOnline.aspx) feature, which allows to directly upload this zip file and will deduce key parameters from it automatically (instead of having to fill them up manually, which is subtly different from the default form you'll get from the menu)

6. The defaults are good, but up to you to change anything you'd like, notably:

    1. Get **HASL lead free** for the **Surface finish**
    2. Pick a **Solder mask** with the color that you like

7. Follow with the order process

    !!! tip "Shipping"
        If you are wondering which carrier option you should take, pick one that offers **DDP**. It means that taxes will be paid in advance and will go through a pre-cleared channel at customs. It looks more expensive now, but your package will not stay stuck at border control for two weeks *and* you won't have to pay duty control fees on top of taxes *and* the tax collection won't be a bloody email circus with Fedex. So essentially it's not only cheaper but also faster and more predictable.

8. Wait a couple of days, and that's it the PCB is in your mail box

## Soldering Stuff

Now the soldering. It should be pretty straightforward. There are four items to solder:

- **Raspberry Pi Pico 2** — Goes into the long rail of holes, the USB port on the same side as the arrow saying "USB this way".
- **DRV8833** — It has a tiny white (when off) LED in the center of one of the edges of the board. There is a "LED" inscription pointing in the direction in which the edge containing this LED should be.
- **2-Pin JST-XH Vertical Header** — It should overlay nicely with the printed shapes on the board. The direction matters, but it should be fairly obvious.
- **4-Pin JST-XH Vertical Header** — Same as the 2-Pin friend.

## Connecting the Fader

So the fader needs two groups of cables. One for the potentiometer (the 4 pins) and one for the motor (the 2 pins). Let's see what/how to wire them.

### 4-pins header — The potentiometer

To connect the potentiometer, you need to build up a custom cable which is on each side:

- 4-Pin JST-XH Female Housing (2.54mm)
- 4-Pin JST-PH Female Housing (2.00mm)

The fader should have a label which says "1 2 T 3", which corresponds to the mapping of the wires next to it. The pins on the board are in the same order.

### 2-pins header — The motor

Okay there I did something stupid. The motor comes with PH connectors (2mm) and I put on the board XH connectors (2.54mm). The reason is that I wanted to use standard distances, but I didn't realize that there was no easy way to adapt this into that.

Things you can do:

- An easy solution is just to cut off the connector from existing cables, and then replace the connector with a XH one.
- Fix the PCB (if you didn't build it yet). I could have done that before publishing the project, but I don't want to publish some untested design so I prefer to keep the mistake and give you the reader options.
- Get an extra breakout board and adapt one onto the other cleanly

The good news however is that the polarity of the motor does not matter, so you can wire this in any combination you want.
