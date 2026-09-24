# Draws Mujina's icons from the mark: two squares, 狢 in the front one (the glyph path in
# ui/assets/mark-glyph.svg). Below 40 px two squares leave the character too small to read,
# so small icons draw the front square alone, filling the icon.
#
#   powershell -NoProfile -File packaging\icons.ps1
#
# Writes docs/images/mujina.svg, the package logos, packaging/mujina.ico (embedded into the
# executables by their build scripts) and the window icons. Windows only: it draws with WPF.

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName PresentationCore, WindowsBase
$root = Split-Path -Parent $PSScriptRoot

$glyphSvg = Get-Content -Raw -Encoding UTF8 "$root\ui\assets\mark-glyph.svg"
$glyph = [regex]::Match($glyphSvg, ' d="([^"]+)"').Groups[1].Value

# The mark in a 64 × 64 box: the back square down-left, the front one up-right, and the
# character centred in the front square at 26/44 of its side, as in the app.
$side = 38.0
$radius = 11.0
$glyphBox = $side * 26 / 44
$glyphScale = $glyphBox / 1000
$glyphX = 21 + $side / 2 - $glyphBox / 2
$glyphY = 9 + $side / 2 - $glyphBox / 2

function Color([byte] $a, [byte] $r, [byte] $g, [byte] $b) {
    [System.Windows.Media.Color]::FromArgb($a, $r, $g, $b)
}
$accent = Color 255 0x91 0x84 0xd9
$accentHalf = Color 128 0x91 0x84 0xd9
$ground = Color 255 0x1a 0x1d 0x2b
$light = Color 255 0xd2 0xce 0xfd

function Brush($color) { New-Object System.Windows.Media.SolidColorBrush $color }

# 狢 with its box of `box` units at (`x`, `y`), in the units of the drawing.
function Glyph([double] $x, [double] $y, [double] $box) {
    $geometry = [System.Windows.Media.Geometry]::Parse($glyph).Clone()
    $place = New-Object System.Windows.Media.TransformGroup
    $place.Children.Add((New-Object System.Windows.Media.TranslateTransform 0, 880))
    $place.Children.Add((New-Object System.Windows.Media.ScaleTransform ($box / 1000), ($box / 1000)))
    $place.Children.Add((New-Object System.Windows.Media.TranslateTransform $x, $y))
    $geometry.Transform = $place
    $geometry
}

# One square PNG of `size` pixels, transparent around the squares.
function Render([int] $size) {
    $scale = $size / 64.0
    $visual = New-Object System.Windows.Media.DrawingVisual
    $dc = $visual.RenderOpen()
    $dc.PushTransform((New-Object System.Windows.Media.ScaleTransform $scale, $scale))
    if ($size -ge 40) {
        # About 1.6 px at the app's size, never thinner than 1.5 px.
        $stroke = [Math]::Max(1.5, $size * 0.022) / $scale
        $back = New-Object System.Windows.Rect 5, 17, $side, $side
        $front = New-Object System.Windows.Rect 21, 9, $side, $side
        # Filled like the front one, as the app's mark looks on its dark rail.
        $dc.DrawRoundedRectangle((Brush $ground), (New-Object System.Windows.Media.Pen (Brush $accentHalf), ($stroke * 0.9)), $back, $radius, $radius)
        $dc.DrawRoundedRectangle((Brush $ground), (New-Object System.Windows.Media.Pen (Brush $accent), $stroke), $front, $radius, $radius)
        $dc.DrawGeometry((Brush $light), $null, (Glyph $glyphX $glyphY $glyphBox))
    } else {
        # One pixel of border, and the character as large as the square allows.
        $stroke = 1.0 / $scale
        $inset = $stroke / 2
        $square = New-Object System.Windows.Rect $inset, $inset, (64 - 2 * $inset), (64 - 2 * $inset)
        $round = 64 * $radius / $side
        $dc.DrawRoundedRectangle((Brush $ground), (New-Object System.Windows.Media.Pen (Brush $accent), $stroke), $square, $round, $round)
        $box = 64 * 0.72
        $dc.DrawGeometry((Brush $light), $null, (Glyph ((64 - $box) / 2) ((64 - $box) / 2) $box))
    }
    $dc.Pop()
    $dc.Close()
    $bitmap = New-Object System.Windows.Media.Imaging.RenderTargetBitmap $size, $size, 96, 96, ([System.Windows.Media.PixelFormats]::Pbgra32)
    $bitmap.Render($visual)
    $encoder = New-Object System.Windows.Media.Imaging.PngBitmapEncoder
    $encoder.Frames.Add([System.Windows.Media.Imaging.BitmapFrame]::Create($bitmap))
    $stream = New-Object System.IO.MemoryStream
    $encoder.Save($stream)
    , $stream.ToArray()
}

function Save-Png([int] $size, [string] $path) {
    [System.IO.File]::WriteAllBytes($path, (Render $size))
}

# The master, for the README and anything that scales. Numbers with a point, whatever the locale.
$invariant = [System.Globalization.CultureInfo]::InvariantCulture
$gx = $glyphX.ToString($invariant)
$gy = $glyphY.ToString($invariant)
$gs = $glyphScale.ToString($invariant)
$svg = @"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <rect x="5" y="17" width="38" height="38" rx="11" fill="#1a1d2b" stroke="#9184d9" stroke-opacity="0.5" stroke-width="1.5"/>
  <rect x="21" y="9" width="38" height="38" rx="11" fill="#1a1d2b" stroke="#9184d9" stroke-width="1.7"/>
  <path fill="#d2cefd" transform="translate($gx $gy) scale($gs) translate(0 880)" d="$glyph"/>
</svg>
"@
New-Item -ItemType Directory -Force "$root\docs\images" | Out-Null
[System.IO.File]::WriteAllText("$root\docs\images\mujina.svg", $svg, (New-Object System.Text.UTF8Encoding $false))

# Package logos. The plain ones are drawn at twice their nominal size, for high-density screens.
$assets = "$root\packaging\Assets"
Get-ChildItem $assets -Filter *.png | Remove-Item
Save-Png 300 "$assets\Square150x150Logo.png"
Save-Png 88 "$assets\Square44x44Logo.png"
Save-Png 100 "$assets\StoreLogo.png"
# The taskbar, Start and the task switcher ask for these exact sizes. "Unplated" makes Windows
# draw the icon as it is, without a coloured plate behind it; "light" is for the light theme.
foreach ($size in 16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256) {
    $image = Render $size
    foreach ($form in "unplated", "lightunplated") {
        [System.IO.File]::WriteAllBytes("$assets\Square44x44Logo.targetsize-${size}_altform-$form.png", $image)
    }
}

# The window icon, for Slint; the apps replace it with the .ico's own sizes once their window
# exists (winutil::window::use_own_icon).
Save-Png 256 "$root\ui\assets\app-icon.png"

# One .ico with every size Windows asks for, each a PNG.
$sizes = 16, 20, 24, 32, 40, 48, 64, 96, 128, 256
$images = foreach ($size in $sizes) { , (Render $size) }
$ico = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter $ico
$writer.Write([uint16] 0)
$writer.Write([uint16] 1)
$writer.Write([uint16] $sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $dimension = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
    $writer.Write([byte] $dimension)
    $writer.Write([byte] $dimension)
    $writer.Write([byte] 0)
    $writer.Write([byte] 0)
    $writer.Write([uint16] 1)
    $writer.Write([uint16] 32)
    $writer.Write([uint32] $images[$i].Length)
    $writer.Write([uint32] $offset)
    $offset += $images[$i].Length
}
foreach ($image in $images) { $writer.Write([byte[]] $image) }
$writer.Flush()
[System.IO.File]::WriteAllBytes("$root\packaging\mujina.ico", $ico.ToArray())
"Icons written."
