SET SCRIPT_DIR=%~dp0

%SCRIPT_DIR%/fontbm/windows/fontbm.exe ^
    --font-file %SCRIPT_DIR%/../art/font/kenney_mini_square_mono.ttf ^
    --font-size 8 ^
    --color 255,0,0 ^
    --monochrome ^
    --output %SCRIPT_DIR%/../art/font/kenney_mini_square_mono
