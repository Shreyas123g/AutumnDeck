import os
import shutil
import subprocess
from PIL import Image

ROOT_DIR = r"C:\Users\rshre\OneDrive\Desktop\develper\Autumn8K\TaskbarHider"
DIST_DIR = os.path.join(ROOT_DIR, "dist")
STAGE_DIR = os.path.join(DIST_DIR, "msix_stage")
ASSETS_DIR = os.path.join(STAGE_DIR, "Assets")
MAKEAPPX = r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.28000.0\x64\makeappx.exe"
VERSION = "2.1.0.0"

# 1. Clean & prepare stage dir
if os.path.exists(STAGE_DIR):
    shutil.rmtree(STAGE_DIR)
os.makedirs(ASSETS_DIR, exist_ok=True)

# 2. Generate required Store icons from AutumnTaskbar_256.png
src_icon_path = os.path.join(ROOT_DIR, "assets", "AutumnTaskbar_256.png")
base_img = Image.open(src_icon_path).convert("RGBA")

# Square 50x50 (StoreLogo)
store_logo = base_img.resize((50, 50), Image.Resampling.LANCZOS)
store_logo.save(os.path.join(ASSETS_DIR, "StoreLogo.png"))

# Square 150x150
logo_150 = base_img.resize((150, 150), Image.Resampling.LANCZOS)
logo_150.save(os.path.join(ASSETS_DIR, "Square150x150Logo.png"))

# Square 44x44
logo_44 = base_img.resize((44, 44), Image.Resampling.LANCZOS)
logo_44.save(os.path.join(ASSETS_DIR, "Square44x44Logo.png"))

# Wide 310x150 (centered icon on transparent canvas)
wide_310 = Image.new("RGBA", (310, 150), (0, 0, 0, 0))
icon_120 = base_img.resize((120, 120), Image.Resampling.LANCZOS)
wide_310.paste(icon_120, ((310 - 120) // 2, (150 - 120) // 2), icon_120)
wide_310.save(os.path.join(ASSETS_DIR, "Wide310x150Logo.png"))

# SplashScreen 620x300 (centered icon on dark slate canvas)
splash = Image.new("RGBA", (620, 300), (20, 20, 25, 255))
icon_180 = base_img.resize((180, 180), Image.Resampling.LANCZOS)
splash.paste(icon_180, ((620 - 180) // 2, (300 - 180) // 2), icon_180)
splash.save(os.path.join(ASSETS_DIR, "SplashScreen.png"))

print("Generated visual assets in Assets/")

# 3. Copy binaries & supporting files
binaries = ["AutumnDeck.exe", "AutumnXamlTap.dll"]
for b in binaries:
    src = os.path.join(ROOT_DIR, b)
    dst = os.path.join(STAGE_DIR, b)
    shutil.copy2(src, dst)
    print(f"Copied {b}")

ico_src = os.path.join(ROOT_DIR, "assets", "AutumnDeck.ico")
if os.path.exists(ico_src):
    shutil.copy2(ico_src, os.path.join(STAGE_DIR, "AutumnDeck.ico"))

mp4_src = os.path.join(ROOT_DIR, "assets", "Autumn_Sanctuary_Starter.mp4")
if os.path.exists(mp4_src):
    wp_dir = os.path.join(STAGE_DIR, "Wallpapers")
    os.makedirs(wp_dir, exist_ok=True)
    shutil.copy2(mp4_src, os.path.join(wp_dir, "Autumn_Sanctuary_Starter.mp4"))

# 4. Write AppxManifest.xml
manifest_content = f"""<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap rescap">

  <Identity
    Name="AutumnDeck.AutumnDeck"
    Publisher="CN=A81A4B13-F326-4336-838D-AE686E1B75C2"
    Version="{VERSION}"
    ProcessorArchitecture="x64" />

  <Properties>
    <DisplayName>AutumnDeck</DisplayName>
    <PublisherDisplayName>AutumnDeck</PublisherDisplayName>
    <Logo>Assets\\StoreLogo.png</Logo>
  </Properties>

  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>

  <Resources>
    <Resource Language="en-us" />
  </Resources>

  <Applications>
    <Application Id="AutumnDeck"
      Executable="AutumnDeck.exe"
      EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
        DisplayName="AutumnDeck"
        Description="Native Windows 11 Taskbar Styler and Customization Suite"
        BackgroundColor="transparent"
        Square150x150Logo="Assets\\Square150x150Logo.png"
        Square44x44Logo="Assets\\Square44x44Logo.png">
        <uap:DefaultTile Wide310x150Logo="Assets\\Wide310x150Logo.png" ShortName="AutumnDeck">
          <uap:ShowNameOnTiles>
            <uap:ShowOn Tile="square150x150Logo" />
          </uap:ShowNameOnTiles>
        </uap:DefaultTile>
        <uap:SplashScreen Image="Assets\\SplashScreen.png" />
      </uap:VisualElements>
    </Application>
  </Applications>

  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
</Package>
"""

manifest_path = os.path.join(STAGE_DIR, "AppxManifest.xml")
with open(manifest_path, "w", encoding="utf-8") as f:
    f.write(manifest_content.strip())
print("Created AppxManifest.xml")

# 5. Build MSIX using makeappx.exe
output_msix = os.path.join(DIST_DIR, f"AutumnDeck_{VERSION}_x64.msix")
if os.path.exists(output_msix):
    os.remove(output_msix)

cmd = [MAKEAPPX, "pack", "/d", STAGE_DIR, "/p", output_msix, "/o"]
print(f"Running makeappx: {' '.join(cmd)}")
res = subprocess.run(cmd, capture_output=True, text=True)
print(res.stdout)
if res.stderr:
    print("Stderr:", res.stderr)
if res.returncode != 0:
    raise RuntimeError(f"makeappx failed with code {res.returncode}")

print(f"MSIX created successfully: {output_msix} ({os.path.getsize(output_msix)} bytes)")

# 6. Copy to Downloads folder and Desktop
downloads_dest = os.path.join(r"C:\Users\rshre\Downloads", f"AutumnDeck_{VERSION}_x64.msix")
shutil.copy2(output_msix, downloads_dest)
print(f"Copied to Downloads: {downloads_dest}")

for desktop_dir in [r"C:\Users\rshre\OneDrive\Desktop", r"C:\Users\rshre\Desktop"]:
    if os.path.exists(desktop_dir):
        desktop_dest = os.path.join(desktop_dir, f"AutumnDeck_{VERSION}_x64.msix")
        shutil.copy2(output_msix, desktop_dest)
        print(f"Copied to Desktop: {desktop_dest}")
