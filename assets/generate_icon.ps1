param(
    [string]$OutIco = (Join-Path $PSScriptRoot "AutumnTaskbar.ico"),
    [string]$OutPreviewPng = (Join-Path $PSScriptRoot "AutumnTaskbar_256.png")
)

Add-Type -AssemblyName System.Drawing

function New-RoundedRectPath {
    param([float]$x, [float]$y, [float]$w, [float]$h, [float]$r)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = [math]::Min($r * 2.0, [math]::Min($w, $h))
    if ($d -le 0.5) {
        $path.AddRectangle([System.Drawing.RectangleF]::new($x, $y, $w, $h))
        return $path
    }
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    return $path
}

function Render-AutumnIcon {
    param([int]$Size)
    $bmp = New-Object System.Drawing.Bitmap($Size, $Size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.Clear([System.Drawing.Color]::Transparent)

    $s = [float]$Size / 256.0

    # 1. Outer squircle card with obsidian-walnut glass gradient
    $pad = 14.0 * $s
    $cw = 256.0 * $s - 2.0 * $pad
    $cr = 56.0 * $s
    $cardPath = New-RoundedRectPath $pad $pad $cw $cw $cr

    $bgBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        [System.Drawing.PointF]::new($pad, $pad),
        [System.Drawing.PointF]::new($pad + $cw, $pad + $cw),
        [System.Drawing.Color]::FromArgb(255, 42, 29, 21),
        [System.Drawing.Color]::FromArgb(255, 16, 12, 10)
    )
    $g.FillPath($bgBrush, $cardPath)
    $bgBrush.Dispose()

    # Soft warm amber radial aura behind center
    $g.SetClip($cardPath)
    $glowPath = New-Object System.Drawing.Drawing2D.GraphicsPath
    $glowPath.AddEllipse(26.0 * $s, 14.0 * $s, 204.0 * $s, 176.0 * $s)
    $glowBrush = New-Object System.Drawing.Drawing2D.PathGradientBrush($glowPath)
    $glowBrush.CenterColor = [System.Drawing.Color]::FromArgb(115, 245, 168, 66)
    $glowBrush.SurroundColors = @([System.Drawing.Color]::FromArgb(0, 245, 130, 42))
    $g.FillPath($glowBrush, $glowPath)
    $glowBrush.Dispose()
    $glowPath.Dispose()

    # Subtle top glass specular reflection
    $specPath = New-RoundedRectPath ($pad + 8.0 * $s) ($pad + 6.0 * $s) ($cw - 16.0 * $s) ($cw * 0.42) ($cr * 0.72)
    $specBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        [System.Drawing.PointF]::new(0, $pad),
        [System.Drawing.PointF]::new(0, $pad + $cw * 0.45),
        [System.Drawing.Color]::FromArgb(52, 255, 242, 216),
        [System.Drawing.Color]::FromArgb(0, 255, 242, 216)
    )
    $g.FillPath($specBrush, $specPath)
    $specBrush.Dispose()
    $specPath.Dispose()

    # 2. Stylized geometric Autumn Sunset / Floating Window Motif in upper center
    $winX = 52.0 * $s
    $winY = 50.0 * $s
    $winW = 152.0 * $s
    $winH = 88.0 * $s
    $winPath = New-RoundedRectPath $winX $winY $winW $winH (22.0 * $s)
    $winFill = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        [System.Drawing.PointF]::new($winX, $winY),
        [System.Drawing.PointF]::new($winX + $winW, $winY + $winH),
        [System.Drawing.Color]::FromArgb(220, 248, 178, 72),
        [System.Drawing.Color]::FromArgb(225, 214, 92, 42)
    )
    $g.FillPath($winFill, $winPath)
    $winFill.Dispose()

    # Dark mountain / horizon silhouette inside desktop preview tile
    $g.SetClip($winPath)
    $mtnPts = [System.Drawing.PointF[]]@(
        [System.Drawing.PointF]::new($winX - 4 * $s, $winY + $winH + 2 * $s),
        [System.Drawing.PointF]::new($winX + 46 * $s, $winY + 46 * $s),
        [System.Drawing.PointF]::new($winX + 94 * $s, $winY + 72 * $s),
        [System.Drawing.PointF]::new($winX + 126 * $s, $winY + 38 * $s),
        [System.Drawing.PointF]::new($winX + $winW + 4 * $s, $winY + $winH + 2 * $s)
    )
    $mtnBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(185, 34, 21, 16))
    $g.FillPolygon($mtnBrush, $mtnPts)
    $mtnBrush.Dispose()

    # Glowing autumn sun disc
    $sunBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(245, 255, 246, 224))
    $g.FillEllipse($sunBrush, $winX + 82.0 * $s, $winY + 14.0 * $s, 26.0 * $s, 26.0 * $s)
    $sunBrush.Dispose()

    $g.SetClip($cardPath)
    $winRimPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(145, 255, 242, 214), [math]::Max(1.0, 2.2 * $s))
    $g.DrawPath($winRimPen, $winPath)
    $winRimPen.Dispose()
    $winPath.Dispose()

    # 3. Signature Floating Autumn Glass Dock / Taskbar Pill at bottom
    $pillX = 36.0 * $s
    $pillY = 158.0 * $s
    $pillW = 184.0 * $s
    $pillH = 50.0 * $s
    $pillPath = New-RoundedRectPath $pillX $pillY $pillW $pillH (24.0 * $s)

    $pillFill = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        [System.Drawing.PointF]::new($pillX, $pillY),
        [System.Drawing.PointF]::new($pillX, $pillY + $pillH),
        [System.Drawing.Color]::FromArgb(245, 48, 36, 28),
        [System.Drawing.Color]::FromArgb(245, 24, 18, 14)
    )
    $g.FillPath($pillFill, $pillPath)
    $pillFill.Dispose()

    $pillPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 245, 172, 72), [math]::Max(1.2, 3.6 * $s))
    $g.DrawPath($pillPen, $pillPath)
    $pillPen.Dispose()
    $pillPath.Dispose()

    # 4 crisp app pips inside the floating dock pill
    $pipColors = @(
        [System.Drawing.Color]::FromArgb(255, 255, 243, 220),
        [System.Drawing.Color]::FromArgb(255, 245, 172, 72),
        [System.Drawing.Color]::FromArgb(255, 232, 109, 58),
        [System.Drawing.Color]::FromArgb(255, 118, 206, 178)
    )
    $pipSize = 22.0 * $s
    $pipGap = 15.0 * $s
    $totalW = 4 * $pipSize + 3 * $pipGap
    $startX = 128.0 * $s - $totalW * 0.5
    $pipY = $pillY + ($pillH - $pipSize) * 0.5
    for ($i = 0; $i -lt 4; $i++) {
        $px = $startX + $i * ($pipSize + $pipGap)
        $pp = New-RoundedRectPath $px $pipY $pipSize $pipSize (7.0 * $s)
        $pb = New-Object System.Drawing.SolidBrush($pipColors[$i])
        $g.FillPath($pb, $pp)
        $pb.Dispose()
        $pp.Dispose()
    }

    $g.ResetClip()

    # Outer 135-degree metallic amber glass rim on squircle card
    $rimBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
        [System.Drawing.PointF]::new($pad, $pad),
        [System.Drawing.PointF]::new($pad + $cw, $pad + $cw),
        [System.Drawing.Color]::FromArgb(240, 255, 210, 128),
        [System.Drawing.Color]::FromArgb(140, 180, 96, 36)
    )
    $rimPen = New-Object System.Drawing.Pen($rimBrush, [math]::Max(1.2, 4.5 * $s))
    $g.DrawPath($rimPen, $cardPath)
    $rimPen.Dispose()
    $rimBrush.Dispose()
    $cardPath.Dispose()

    $g.Dispose()
    return $bmp
}

$sizes = @(16, 24, 32, 48, 64, 128, 256)
$pngBlobs = @()
foreach ($sz in $sizes) {
    $bmp = Render-AutumnIcon -Size $sz
    if ($sz -eq 256 -and $OutPreviewPng) {
        $bmp.Save($OutPreviewPng, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $pngBlobs += ,($sz, $ms.ToArray())
    $ms.Dispose()
    $bmp.Dispose()
}

# Assemble multi-resolution PNG-in-ICO file
$fs = [System.IO.File]::Create($OutIco)
$bw = New-Object System.IO.BinaryWriter($fs)
# ICONDIR: reserved(2)=0, type(2)=1 (icon), count(2)
$bw.Write([uint16]0)
$bw.Write([uint16]1)
$bw.Write([uint16]$pngBlobs.Count)

$offset = 6 + (16 * $pngBlobs.Count)
foreach ($entry in $pngBlobs) {
    $sz = [int]$entry[0]
    $bytes = [byte[]]$entry[1]
    $wByte = if ($sz -ge 256) { [byte]0 } else { [byte]$sz }
    $bw.Write($wByte)         # bWidth
    $bw.Write($wByte)         # bHeight
    $bw.Write([byte]0)        # bColorCount
    $bw.Write([byte]0)        # bReserved
    $bw.Write([uint16]1)      # wPlanes
    $bw.Write([uint16]32)     # wBitCount
    $bw.Write([uint32]$bytes.Length)
    $bw.Write([uint32]$offset)
    $offset += $bytes.Length
}
foreach ($entry in $pngBlobs) {
    $bw.Write([byte[]]$entry[1])
}
$bw.Flush()
$bw.Dispose()
$fs.Dispose()
Write-Host "ICON_OK path=$OutIco size=$((Get-Item $OutIco).Length)"
