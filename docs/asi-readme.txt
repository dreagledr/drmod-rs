drmod
In-engine mod for Metal Gear Rising: Revengeance (PC, Steam).

WHAT IS IN THIS ARCHIVE
  plugins\drmod_rs_lib.asi - the mod itself. That is all the game loads;
  this readme.txt is only for you and can be deleted.
  d3d9.dll - an ASI loader. The game does not load .asi plugins by itself,
  so something has to.

INSTALL
  1. Copy d3d9.dll and the plugins folder into the game's root folder, so that
     the tree looks like this:

       Metal Gear Rising REVENGEANCE\
       |-- METAL GEAR RISING REVENGEANCE.EXE
       |-- d3d9.dll
       |-- plugins\
            |-- drmod_rs_lib.asi

  2. Start the game as usual. The mod loads with it - no launcher, no
     injection.

  If you already have a d3d9.dll there (ReShade, ENB, another ASI mod), keep
  YOUR file and copy only plugins\drmod_rs_lib.asi. Any ASI loader loads this
  plugin; it does not have to be the one in this archive.

  Do not have a loader at all? Take the latest Win32 d3d9.dll from
  https://github.com/ThirteenAG/Ultimate-ASI-Loader/releases

UNINSTALL
  Delete plugins\drmod_rs_lib.asi, and d3d9.dll unless another mod needs that
  loader.

ALTERNATIVE
  drmod.zip ships drmod.exe - a launcher that starts the game and injects the
  very same mod, so no loader and no plugins folder are needed. It is the
  smaller download; the ASI form is the one that needs no second process.
