# 3D Printing

The PCB is of course going to be a bit naked for regular use, which is why I also created a bunch of parts.

My desk is equipped with a couple of [GEN2](https://www.jerrari3d.com/gen2-modular-system) drawers (and associated rails), which is a fantastic drawer system might I add, and thus is perfect to attach this kind of projects as well. Which brought me to the conclusion: let's make a GEN2-drawer-shaped controller, this way I can stick it in a standard case and benefit from having the system already in place. The side effect of this decision is that you are going to need a GEN2 drawer rail and case in place to be able to mount the project, but more on that later.

!!! note
    In case you need to change anything about the 3D models, you can always clone the [Onshape document](https://cad.onshape.com/documents/e0a1011ad933c9838ad313ca/w/9072f4ad6800d0c6b8320443/e/6ad28f5df8e4bbfb02ddf18f?renderMode=0&uiState=69ffd1ceabeaa0112793777b) to customize anything you would like. I've tried to make it understandable, and to have one single place to change any detail and get it rippling everywhere else automatically. For example if you modify the fader's ghost, it should automatically re-locate the holes and so forth on the facade.

## Parts

We have there to print the following parts:

| Part | Preview | STL | Purpose |
|------|---------|-----|---------|
| **Drawer** | ![](../img/stl/DHD_Drawer.png){ width="120" } | [:octicons-download-16: Download](https://github.com/Xowap/dhd/raw/develop/hardware/stl/DHD_Drawer.stl) | The support structure for everything on top of being the compatibility layer with GEN2 |
| **Facade** | ![](../img/stl/DHD_Facade.png){ width="120" } | [:octicons-download-16: Download](https://github.com/Xowap/dhd/raw/develop/hardware/stl/DHD_Facade.stl) | The main component, and for which you could create a different support system if you don't like GEN2 (all you need is 4 holes and somewhere to put the PCB) |
| **Side Panels** | ![](../img/stl/DHD_Panel_Left.png){ width="120" } | [:octicons-download-16: Left](https://github.com/Xowap/dhd/raw/develop/hardware/stl/DHD_Panel_Left.stl) · [:octicons-download-16: Right](https://github.com/Xowap/dhd/raw/develop/hardware/stl/DHD_Panel_Right.stl) | Only here for cosmetic reasons and close up the sides of the facade. Albeit technically optional, I don't really see the point of not printing them |
| **Knob** | ![](../img/stl/DHD_Knob.png){ width="120" } | [:octicons-download-16: Download](https://github.com/Xowap/dhd/raw/develop/hardware/stl/DHD_Knob.stl) | You put it on the sliding part of the fader, and gives you a studio feel (or at least is more comfortable to use than a rectangular piece of metal) |

## Color Strategy

- The drawer is cased away during normal use, so print it with anything you'd like but essentially you'll never actually see it
- The facade is obviously the main thing that you are going to be seeing, so print with a material that you like. Also take into account that there are two pretty prominent countersunk screws on it, so you might want to match up the color of the drawer and the color of the screws
- The side panels are going to be visible on the side, so I guess you can give them an accent color which goes well with the facade

## Printing Strategy

Everything prints without supports, although you might want to take this into consideration:

- **The drawer** prints flat without any surprises, although to be extra safe you might want a brim or at least some mouse ears
- **The facade** is relatively simple to print but you might want to decide which face to print it on:
    - My recommendation is to print it on one of the sides, as the only complication is a reasonable bridge which prints a bit soggy but stays hidden. So essentially, printing on the side is foolproof.
    - On the other hand, you might want fancy surface finish on the front face. In which case there is no major reason not to print it on the front face, except for the looong bridge it will create. But then again, it's on the back side and you won't ever see it, and there is ample margin around the fader.
- **The side panels** can be printed either:
    - On the outer face, which is by far the simplest. Depends the finish you want, but I'd go for that.
    - You can however also print it on the "support" (the bit with the screw hole), but the contact surface is pretty tiny and the shape prone to warping so you're gonna have to be careful about adhesion. Not sure why you would do that.
- **The knob** has to be printed on its back face, for better control over the tolerances inside (otherwise it's going to be bridging), which are honestly tight (on purpose, you don't want it to fall off do you?). Which sucks in terms of layer lines orientation, but is simpler. If you want to try it otherwise let me know how it goes.
