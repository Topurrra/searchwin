# The app icon: the same white plate and the same mark as Icon/icon.swift in
# the Mac app, drawn with System.Drawing and packed into one .ico holding every
# size Windows asks for. Run it again only if the mark changes.
#
#   pwsh -File Icon/icon.ps1

param([string]$Out = (Join-Path $PSScriptRoot '..\Search\Assets\Search.ico'))

Add-Type -AssemblyName System.Drawing

$mark = 'M469.443 0C545.471 0.00013198 607.103 61.6325 607.104 137.66C607.104 213.688 545.471 275.321 469.443 275.321H137.66C61.6323 275.321 0 213.688 0 137.66C0.00016085 61.6325 61.6325 0.000140192 137.66 0H469.443ZM138.104 51.5977C127.234 51.5977 117.512 53.5115 108.938 57.3389C100.518 61.0132 93.8581 66.2188 88.959 72.9551C84.2132 79.5381 81.8398 87.3464 81.8398 96.3789C81.8399 105.258 83.6773 112.607 87.3516 118.425C91.0258 124.089 95.9251 128.682 102.049 132.203C108.173 135.571 114.833 138.327 122.028 140.471L151.652 149.197C158.389 151.188 163.9 154.249 168.187 158.383C172.473 162.516 174.617 168.028 174.617 174.917C174.617 182.572 171.402 188.849 164.972 193.748C158.695 198.494 150.122 200.867 139.252 200.867C132.21 200.867 125.702 199.413 119.731 196.504C113.914 193.442 109.091 189.308 105.264 184.103C101.436 178.744 99.2169 172.697 98.6045 165.961H97.6855L75.4102 171.013C76.3287 180.658 79.697 189.308 85.5146 196.963C91.3322 204.618 98.9103 210.665 108.249 215.104C117.741 219.544 128.076 221.765 139.252 221.765C151.193 221.765 161.68 219.774 170.713 215.794C179.746 211.813 186.711 206.225 191.61 199.029C196.662 191.834 199.188 183.414 199.188 173.769C199.188 164.124 197.352 156.239 193.678 150.115C190.003 143.838 185.104 138.863 178.98 135.188C172.857 131.514 166.044 128.605 158.542 126.462L128.229 117.735C121.799 115.898 116.516 113.219 112.383 109.698C108.402 106.177 106.412 101.354 106.412 95.2305C106.412 88.188 109.168 82.6758 114.68 78.6953C120.344 74.5619 128.152 72.4951 138.104 72.4951C147.901 72.4952 155.939 74.9448 162.216 79.8438C168.493 84.7428 172.397 91.1729 173.928 99.1338H174.847L196.663 93.8525C195.745 85.5853 192.605 78.3131 187.247 72.0361C181.889 65.6061 174.923 60.6306 166.35 57.1094C157.929 53.4351 148.514 51.5977 138.104 51.5977Z'

# Absolute M, L, H, V, C, Z — what Figma writes for a flattened shape.
function Read-Commands([string]$d) {
    $out = @()
    foreach ($m in [regex]::Matches($d, '([MLHVCZ])([^MLHVCZ]*)')) {
        $nums = @([regex]::Matches($m.Groups[2].Value, '-?\d*\.?\d+(?:e-?\d+)?') | ForEach-Object { [double]$_.Value })
        $out += , @($m.Groups[1].Value, $nums)
    }
    $out
}

function New-MarkPath([System.Drawing.RectangleF]$plate, [double]$fraction) {
    $scale = $plate.Width * $fraction / 608.0
    $ox = $plate.X + $plate.Width / 2 - 608.0 * $scale / 2
    $oy = $plate.Y + $plate.Height / 2 - 276.0 * $scale / 2
    $pt = { param($x, $y) [System.Drawing.PointF]::new([float]($ox + $x * $scale), [float]($oy + $y * $scale)) }
    $path = [System.Drawing.Drawing2D.GraphicsPath]::new([System.Drawing.Drawing2D.FillMode]::Alternate)
    $last = @(0.0, 0.0)
    foreach ($c in (Read-Commands $mark)) {
        $n = $c[1]
        switch ($c[0]) {
            'M' { $path.StartFigure(); $last = @($n[0], $n[1]) }
            'L' { $path.AddLine((& $pt $last[0] $last[1]), (& $pt $n[0] $n[1])); $last = @($n[0], $n[1]) }
            'H' { $path.AddLine((& $pt $last[0] $last[1]), (& $pt $n[0] $last[1])); $last = @($n[0], $last[1]) }
            'V' { $path.AddLine((& $pt $last[0] $last[1]), (& $pt $last[0] $n[0])); $last = @($last[0], $n[0]) }
            'C' {
                for ($k = 0; $k + 5 -lt $n.Count; $k += 6) {
                    $path.AddBezier((& $pt $last[0] $last[1]), (& $pt $n[$k] $n[$k + 1]), (& $pt $n[$k + 2] $n[$k + 3]), (& $pt $n[$k + 4] $n[$k + 5]))
                    $last = @($n[$k + 4], $n[$k + 5])
                }
            }
            'Z' { $path.CloseFigure() }
        }
    }
    $path
}

function Draw([int]$px) {
    $bmp = [System.Drawing.Bitmap]::new($px, $px, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.PixelOffsetMode = 'HighQuality'
    # Windows icons run nearer the edge than the Mac's, which leaves room
    # for a shadow the taskbar never shows.
    $inset = [Math]::Max(1, $px * 0.06)
    $plate = [System.Drawing.RectangleF]::new($inset, $inset, $px - 2 * $inset, $px - 2 * $inset)
    $r = $plate.Width * 0.2237
    $shape = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $shape.AddArc($plate.X, $plate.Y, 2 * $r, 2 * $r, 180, 90)
    $shape.AddArc($plate.Right - 2 * $r, $plate.Y, 2 * $r, 2 * $r, 270, 90)
    $shape.AddArc($plate.Right - 2 * $r, $plate.Bottom - 2 * $r, 2 * $r, 2 * $r, 0, 90)
    $shape.AddArc($plate.X, $plate.Bottom - 2 * $r, 2 * $r, 2 * $r, 90, 90)
    $shape.CloseFigure()
    $g.FillPath([System.Drawing.SolidBrush]::new([System.Drawing.Color]::White), $shape)
    $g.DrawPath([System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(40, 0, 0, 0), [float][Math]::Max(1, $px / 256)), $shape)
    $g.FillPath([System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 23, 23, 23)), (New-MarkPath $plate 0.754))
    $g.Dispose()
    $bmp
}

$sizes = 16, 20, 24, 32, 40, 48, 64, 128, 256
$images = foreach ($s in $sizes) {
    $bmp = Draw $s
    $ms = [System.IO.MemoryStream]::new()
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , $ms.ToArray()
}

# ICONDIR, one ICONDIRENTRY per size, then the PNGs themselves.
$fs = [System.IO.File]::Create((Resolve-Path -LiteralPath (Split-Path $Out) | Join-Path -ChildPath (Split-Path $Out -Leaf)))
$w = [System.IO.BinaryWriter]::new($fs)
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $bytes = $images[$i]
    $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s }))); $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
    $w.Write([byte]0); $w.Write([byte]0); $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$bytes.Length); $w.Write([uint32]$offset)
    $offset += $bytes.Length
}
foreach ($bytes in $images) { $w.Write($bytes) }
$w.Close()
"drew: $Out"
