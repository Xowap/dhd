# DHD &mdash; Construction guide

The Dial Hifi Device (DHD) is a DIY device which allows you to control the volume on your computer through a physical fader, which is mounted as the facade of a drawer for the GEN2 system. Obviously it's built with the parts that *I* have at hands, and building it for your own constraints might require to modify some things. Overall everything is open sourced: you have access to the Onshape document, the KiCad schematics and the Rust source code for the computer program.

<img src="/home/remy/dev/dhd/doc/img/assembly-view.png" style="zoom: 25%;" />

## Prerequisites

In order to build this device, you will need at least some experience with 3D printing and soldering, even though I'm trying to make this as easy as possible.

### Tools

These are the tools that you will need in order to assemble the product from start to finish (let me know if I forgot something):

- Required
  - **Soldering Iron** &mdash; Used both for soldering and for heat-set inserts insertion
  - **3D Printer** &mdash; In order to print the printable parts
- Recommended
  - **Hex screwdrivers/keys** &mdash; Probably sizes 2 and/or 2.5. You can always screw using your fingers only but that's definitely not the easiest way to go.
  - **Crimping tool** &mdash; To work with the JST connectors, you will need a crimping tool. You can always work with regular pliers and cry a lot if you don't have one.
  - **Automatic Wire Stripper** &mdash; Same as above. Those wires are tiny. But if you're good with a blade...

### Commodities

You will also need to consume a few things to get going:

- **Some filament** for your 3D printer (colors up to you, any PLA is fine)
- **Flux** and **wick** for soldering
- **2.54mm PCB Pin Header** to solder the Raspberry Pi and the driver, unless they already have some in the box

### Hardware

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
| 4-Pin JST-PH Calbes with Male Plug Connector (2.00mm) | Cable           | 1    | [ASIN: B0C9C9VF4T](https://amzn.eu/d/00JlXiUl)               |
| M3*6 hex socket head cap screw                        | Screw           | 8    | [ASIN: B0DQ19XG23](https://amzn.eu/d/08OMiqco)               |
| M3*8 hex socket countersunk head screw                | Screw           | 4    | [ASIN: B0DPWZ53X4](https://amzn.eu/d/03UlMjSr)               |
| M3x4.0x5.0 heat-set insert                            | Heat-set insert | 2    | [ASIN: B0B2DMWFT2](https://amzn.eu/d/09NEdmbo)               |
| M3x4.8x5.0 heat-set insert                            | Heat-set insert | 4    | [ASIN: B0B2DMWFT2](https://amzn.eu/d/09NEdmbo)               |
| M3x5.7x5.0 heat-set insert                            | Heat-set insert | 4    | [ASIN: B0B2DMWFT2](https://amzn.eu/d/09NEdmbo)               |

> *Note* &mdash; The "what I bought" links are not affiliated, and as a matter of fact most of them are not buyable anymore. But at least this way you can see exactly what I'm referring to.

#### Operating margin

Overall, I understand that you might not find where you are exactly what is listed above, depending on the country in which you are.

- **The fader** &mdash; As long as you have a fader with a 2-wired motor and the "1 2 3" pins, you should be good. We're not using the "T" pin, so you don't have to wire it in the connector if you don't have it. The software makes as few assumptions as possible, so any fader should be compatible. The design [in Onshape](https://cad.onshape.com/documents/e0a1011ad933c9838ad313ca/w/9072f4ad6800d0c6b8320443/e/6ad28f5df8e4bbfb02ddf18f?renderMode=0&uiState=69ffd1ceabeaa0112793777b) is also configured to be easily modified to fit a differently-sized fader.
- **Raspberry Pi Pico 2** &mdash; We don't do anything crazy, any Raspberry Pi Pico should do the job. The only thing is, it's totally untested. And even less tested with the W version, which has Wifi. Although there are no reasons to think it would fail.
- **DRV8833** &mdash; My understanding is that the breakout board is not standard for this component. So obviously you'll either have to find the same one or to re-wire the PCB.
- **JST connectors** &mdash; The PSL60 uses 2mm pins and the board 2.54mm pins. I'm counting on the cables to do the "translation" (see below). Change the size of the headers and you'll need to update the PCB a bit.
- **Screws** and **heat-set inserts** &mdash; Most places allow for longer screws or inserts without any major issue. Even a shorter insert is hardly ever going to be a problem, as the mechanical properties required for this assembly are very achievable. The main thing to be mindful of is to have sufficiently long screws, otherwise they risk not screwing anything. Also the screw heads (countersunk vs head cap) have their importance but then again it *would* hold even with the wrong head, if you're not afraid of ugly.

## The PCB

My understanding is that there are many ways to do this, however here is how I did it.

Within the source code, under the `hardware/kicad` folder, you can find the schematics for the PCB, to be opened in the free software [KiCad](https://www.kicad.org/).

### Building the board

> Before building the board, read the instructions of the "2-pins header &mdash; The motor" below, because it might impact your process.

Then, up to you. If you know how to do this chemically or with a laser or whatever the cool kids do, knock yourself out. In my case, I decided to order it from PCBWays, because it's much easier. Here are the steps that I followed:

1. Open the PCB in KiCad

2. (Export gerber)

3. (Export drill)

4. Put all those files at the root of a `.zip`

5. Go to PCBWay and use the [PCB Quick Order](https://www.pcbway.com/QuickOrderOnline.aspx) feature, which allows to directly upload this zip file and will deduce key parameters from it automatically (instead of having to fill them up manually, which is subtly different from the default form you'll get from the menu)

6. The defaults are good, but up to you to change anything you'd like, notably:

   1. Get **HASL lead free** for the **Surface finish**
   2. Pick a **Solder mask** with the color that you like

7. Follow with the order process

   > If you are wondering which carrier option you should take, pick one that offers **DDP**. It means that taxes will be paid in advance and will go through a pre-cleared channel at customs. It looks more expensive now, but your package will not stay stuck at border control for two weeks *and* you won't have to pay duty control fees on top of taxes *and* the tax collection won't be a bloody email circus with Fedex. So essentially it's not only cheaper but also faster and more predictable.

8. Wait a couple of days, and that's it the PCB is in your mail box

### Soldering stuff

Now the soldering. It should be pretty straightforward. There are four items to solder:

- **Raspberry Pi Pico 2** &mdash; Goes into the long rail of holes, the USB port on the same side as the arrow saying "USB this way".
- **DRV8833** &mdash; It has a tiny white (when off) led in the center of one of the edges of the board. There is a "LED" inscription pointing in the direction in which the edge containing this LED should be.
- **2-Pin JST-XH Vertical Header** &mdash; It should overlay nicely with the printed shapes on the board. The direction matters, but it should be fairly obvious.
- **4-Pin JST-XH Vertical Header** &mdash; Same as the 2-Pin friend.

### Connecting the fader

So the fader needs two groups of cables. One for the potentiometer (the 4 pins) and one for the motor (the 2 pins). Let's see what/how to wire them.

#### 4-pins header &mdash; The potentiometer

To connect the potentiometer, you need to build up a custom cable which is on each side:

- 4-Pin JST-XH Female Housing (2.54mm)
- 4-Pin JST-PH Female Housing (2.00mm)

The fader should have a label which says "1 2 T 3", which corresponds to the mapping of the wires next to it. The pins on the board are in the same order, (explain here how to figure the order).

#### 2-pins header &mdash; The motor

Okay there I did something stupid. The motor comes with PH connectors (2mm) and I put on the board XH connectors (2.54mm). The reason is that I wanted to use standard distances, but I didn't realize that there was no easy way to adapt this into that.

Things you can do:

- An easy solution is just to cut off the connector from existing cables, and then replace the connector with a XH one.
- Fix the PCB (if you didn't build it yet). I could have done that before publishing the project, but I don't want to publish some untested design so I prefer to keep the mistake and give you the reader options.
- Get an extra breakout board and adapt one onto the other cleanly

The good news however is that the polarity of the motor does not matter, so you can wire this in any combination you want.

## Printing the parts

The PCB is of course going to be a bit naked for regular use, which is why I also created a bunch of parts.

My desk is equipped with a couple of [GEN2](https://www.jerrari3d.com/gen2-modular-system) drawers (and associated rails), which is a fantastic drawer system might I add, and thus is perfect to attach this kind of projects as well. Which brought me to the conclusion: let's make a GEN2-drawer-shaped controller, this way I can stick it in a standard case and benefit from having the system already in place. The side effect of this decision is that you are going to need a GEN2 drawer rail and case in place to be able to mount the project, but more on that later.

> *Note* &mdash; In case you need to change anything about the 3D models, you can always clone the [Onshape document](https://cad.onshape.com/documents/e0a1011ad933c9838ad313ca/w/9072f4ad6800d0c6b8320443/e/6ad28f5df8e4bbfb02ddf18f?renderMode=0&uiState=69ffd1ceabeaa0112793777b) to customize anything you would like. I've tried to make it understandable, and to have one single place to change any detail and get it rippling everywhere else automatically. For example if you modify the fader's ghost, it should automatically re-locate the holes and so forth on the facade.

We have there to print the following parts:

- **Drawer** itself, which is the support structure for everything on top of being the compatibility layer with GEN2
- **Facade** which is the main component, and for which you could create a different support system if you don't like GEN2 (all you need is 4 holes and somewhere to put the PCB)
- **Side panels** who are only here for cosmetic reasons and close up the sides of the facade. Albeit technically optional, I don't really see the point of not printing them
- **Knob** which you put on the sliding part of the fader, and gives you a studio feel (or at least is more comfortable to use than a rectangular piece of metal)

In terms of color strategy:

- The drawer is cased away during normal use, so print it with anything you'd like but essentially you'll never actually see it
- The facade is obviously the main thing that you are going to be seeing, so print with a material that you like. Also take into account that there are to pretty prominent countersunk screws on it, so you might want to match up the color of the drawer and the color of the screws
- The side panels are going to be visible on the side, so I guess you can give them an accent color which goes well with the facade

In terms of printing strategy, everything prints without supports, although you might want to take this into consideration:

- The drawer prints flat without any surprises, although to be extra safe you might want a brim or at least some mouse ears
- The facade is relatively simple to print but you might want to decide which face to print it on:
  - My recommendation is to print it on one of the sides, as the only complication is a reasonable bridge which prints a bit soggy but stays hidden. So essentially, printing on the side is foolproof.
  - On the other hand, you might want fancy surface finish on the front face. In which case there is no major reason not to print it on the front face, except for the looong bridge it will create. But then again, it's on the back side and you won't ever see it, and there is ample margin around the fader.
- The side panels can be printed either:
  - On the outer face, which is by far the simplest. Depends the finish you want, but I'd go for that.
  - You can however also print it on the "support" (the bit with the screw hole), but the contact surface is pretty tiny and the shape prone to warping so you're gonna have to be careful about adhesion. Not sure why you would do that.
- The knob has to be printed on its back face, for better control over the tolerances inside (otherwise it's going to be bridging), which are honestly tight (on purpose, you don't want it to fall off do you?). Which sucks in terms of layer lines orientation, but is simpler. If you want to try it otherwise let me know how it goes.

## The assembly

Now is the time to assemble all that you have gathered and built.

### The BOM

Okay so to start, what should you have on your table? This list is different from the one above, as some elements already have been assembled and others have been printed.

| Item                                   | Type            | Qty  |
| -------------------------------------- | --------------- | ---- |
| Bourns PSL60                           | Motorized fader | 1    |
| 4-Pin JST-PH to JST-XH cable           | Cable           | 1    |
| M3*6 hex socket head cap screw         | Screw           | 8    |
| M3*8 hex socket countersunk head screw | Screw           | 4    |
| M3x4.0x5.0 heat-set insert             | Heat-set insert | 2    |
| M3x4.8x5.0 heat-set insert             | Heat-set insert | 4    |
| M3x5.7x5.0 heat-set insert             | Heat-set insert | 4    |
| Drawer                                 | Printed part    | 1    |
| Facade                                 | Printed part    | 1    |
| Left panel                             | Printed part    | 1    |
| Right panel                            | Printed part    | 1    |
| Knob                                   | Printed part    | 1    |
| Soldered PCB                           | PCB             | 1    |

### Phase A &mdash; Heat-set inserts

Grab your soldering iron, set it to about 50°C above printing temperature (so something like 270°C for PLA) and push the inserts into the plastic. This design is rather tolerant for different sizes of inserts, longer or shorter. Just make sure that the contact surface is flush so that the parts can be fastened closely.

#### Step 1 &mdash; Insert into the panels

Push the M3x4.0x5.0 inserts into the panel's bottom holes. It can overflow in direction of the pointy end, but the bottom surface must be flush.

<img src="/home/remy/dev/dhd/doc/img/assembly_step-1.png" style="zoom: 25%;" />

#### Step 2 &mdash; Insert into the facade

Push the M3x4.8x5.0 inserts into the facade's back face. It can overflow inside of the facade, but the back face must be flush.

<img src="/home/remy/dev/dhd/doc/img/assembly_step-2.png" style="zoom:25%;" />

#### Step 3 &mdash; Insert into the PCB support

Push the M3x5.7x5.0 inserts into the drawer's PCB support elements. If the surface isn't flush after insertion, it's not too dramatic as the PCB will be screwed in there.

<img src="/home/remy/dev/dhd/doc/img/assembly_step-3.png" style="zoom:25%;" />

### Phase B &mdash; Facade assembly

The facade is the main part of this project, and almost works as an independent module. Here we assemble its components. The next phase will essentially only be to screw it to the drawer.

#### Step 4 &mdash; Screw the panels

Position the panels more or less by pushing them from the side, then when their hole is sufficiently aligned with the screw hole from below, screw them with 2 M3*8 hex socket countersunk head screws.

<img src="/home/remy/dev/dhd/doc/img/assembly_step-4.png" style="zoom:25%;" />

#### Step 5 &mdash; Screw the fader

Here is represented the fader's "hitbox", which is slightly different from the actual fader, but I wasn't going to make a CAD model of it. Anyways, as you can see the socket for the potentiometer sensors is pointing up and the motor is on the left side if you look from the front. Use the other two M3*8 hex socket countersunk head screws.

<img src="/home/remy/dev/dhd/doc/img/assembly_step-5.png" style="zoom:25%;" />

#### Step 6 &mdash; Attach the knob

The knob just needs to be pushed into the fader's metal slider. If you observe the metal thing close enough, you will see that two circles are pressed on each side. Inside the knob's hole you should find the negative of these shapes. Take care to insert the knob in the right direction so that the relief on one side inserts itself in the hole inside the knob. Adjustments are very tight, so you may have to use a bit of force, although nothing too dramatic. It does not exactly click, but you might feel something when it gets into position. The knob should sit about 1mm from the front face.

<img src="/home/remy/dev/dhd/doc/img/assembly_step-6.png" style="zoom:25%;" />

### Phase C &mdash; Bring it together

Now it's a simple question of connecting it all together.

#### Step 7 &mdash; Screw the PCB

Be careful about the orientation of the PCB. Basically the Raspberry Pi's USB should point towards the right side, which is the side on which there is lots of space for the cable to hang. In doubt, look at the holes from the schema and line them up with what you have. Use 4 M3*6 hex socket head cap screws.

<img src="/home/remy/dev/dhd/doc/img/assembly_step-7.png" style="zoom:25%;" />

#### Step 8 &mdash; Screw the facade to the drawer

Use the remaining 4 M3*6 hex socket head cap screws for this.

<img src="/home/remy/dev/dhd/doc/img/assembly_step-8.png" style="zoom:25%;" />

#### Step 9 &mdash; Connect the cables

Now is time to connect the two cables that you have:

- Connect the 4 pin of the fader to the 4 pin of the PCB
- And connect the 2 pin of the motor to the 2 pin of the PCB

## Putting it into GEN2

Not to cover the whole GEN2 documentation, but let's review the models that you will need.

Shortly put, the produced drawer is:

- Compatible with the 185 variant (which is the depth of the drawer, the most common)
- Dimensions are 2Wx1H

First of all, if you want to do it like me, you'll need the under-desk rail. Get the [GEN2-QL Rail - Double](https://www.printables.com/model/1052357-gen2-rails-185-standard/files) file. Once printed you'll need self-taping screws to screw it to the desk from underneath. I used [M4x20mm wood screws](https://amzn.eu/d/0ekTw1Um) for my Ikea desk, obviously measure before doing anything.

Then you'll need the case. Basically cases are stacked underneath the rail, and have a rail of their own below so that you can append more cases below. For our drawer, we'll need the [185-2W-1H Case](https://www.printables.com/model/1658700-gen2-185-cases-all/files).

> *Note* &mdash; If you already have cases, make sure they are recent. For the USB to be able to go through the back, I'm using the wall attachment hole, which appeared on the 1st of April 2026. So if you've got an old file, it's not going to work out. As a corollary, you can't attach your case to the wall with this drawer, at least not without modifying it (and the case) a bit.

## Using it

For now the software distribution is not fantastic, in the sense that you will also have to build it yourself.

First of all, it only works for Linux. So for other operating systems, you can probably vibe-code it pretty easily given that most of the code is portable, however yeah that's on you.

The things you'll need to get started are:

- `git`, of course
- and `rust`, so for example you can get [`rustup`](https://rustup.rs/)

Start by cloning the repo:

```bash
git clone https://github.com/Xowap/dhd.git
cd dhd
```

Then install the toolchain for cross-compilation:

```
rustup target add thumbv8m.main-none-eabihf
```

Then there are two components:

- The `firmware`, which runs on the device
- And the `host`, which runs on your computer and that is the Linux-only part

### Flashing the firmware

So now, let's flash this firmware.

Look at your Raspberry Pi Pico 2. It has a `BOOTSEL` button. Maintain it pressed while plugging it to the USB port of your computer. This puts the device in "flashable" mode.

Subsequently, build and flash the firmware:

```bash
cd firmware
cargo run --release
cd ..
```

This will transfer the firmware to the device and then it should immediately turn on. Of course it does nothing on its own so you need to start the host software.

### Starting the host

The host works in CLI mode, however if you intend on using it I recommend using the self-installer.

```bash
cd host
cargo run --release -- install
```

Doing so will:

- Install the binary in `~/.local/bin/dhd`
- Configure the application to appear in your menu and to start automatically at system launch

The installed files are listed, so if you want to cleanup afterwards it's very simple to do because you just need to delete them.

Now that the software is installed:

- Make sure nothing is obstructing your knob. The calibration process will start immediately when the host program turns on
- Look for the "DHD Host" in your menu to start the program

You will get a small icon appearing in your systray, which tells you the state at all times.

As said before, when starting for the first time a calibration process happens. Don't worry, the results are saved on the device itself and remembered after reboot, so this will only happen once. You can however force a re-calibration for any reason (new knob with different weight, first calibration got interfered with, etc) by right-clicking the systray icon and selecting "Force Calibration".

Another thing which you will probably need to do if you have the same setup as me, is that the potentiometer is mounted with 0% on the right and 100% on the left, which in many cultures is counter-intuitive for volume setting. In which case you can opt to invert the scale, and you will be left-to-right.

The software makes few assumptions regarding the hardware, this way you can potentially hook up another fader out-of-the-box (modulo the exact screw locations) and it's likely that it will work decently. No alternative has been tested however.
