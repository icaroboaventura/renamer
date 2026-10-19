# Renamer icon · matte utility family

Created with the built-in image generation tool. Final source: `renamer-icon.png`; macOS asset: `Renamer.icns`.

## Reusable direction

Use one simple central object, soft matte satin ceramic material, rounded geometry, warm ivory background, periwinkle blue and lilac palette, upper-left studio lighting, and subtle contact shadows. No text or lettering. For another tool, keep this image as a style reference and change only the symbolic object.

## Generation prompt

Use case: stylized-concept. Asset: final macOS desktop app icon for Renamer, establishing a consistent family of personal utility icons. Square 1024x1024 composition. A warm off-white rounded-square app tile with generously rounded corners, centered and almost filling the canvas, transparent outside the tile. On the tile, two chunky rounded luggage/file labels overlapping: rear periwinkle-blue tag tilted gently left and front soft lilac tag tilted gently right, each with a simple small circular punched hole, no strings. A single small but bold white curved right-pointing arrow embossed on the front tag represents renaming. The tags occupy 65 percent of the canvas. Premium simple 3D matte satin ceramic material, broad rounded bevels, front view with very slight perspective, soft upper-left studio light, short subtle contact shadows. Quiet, friendly, clean geometry, crisp silhouette recognizable at small Dock sizes. No text, no letters, no numbers, no watermark, no extra objects, no glossy glass or metallic reflections. Keep the complete tile and shadow inside canvas with a narrow transparent margin.

## Final refinement prompt

Keep the central blue and lilac matte 3D tags and white arrow exactly as shown. Replace the entire background, including every outside edge and corner, with a single solid warm ivory background #FAF7F0 extending all the way to all four square canvas edges. Opaque image, no transparency anywhere. No rounded-square tile boundary: seamless flat ivory background only, retaining soft contact shadows directly behind the tags. Preserve the artwork scale and colors. No text or extra details.

## Packaging

The original PNG remains at generated resolution. macOS icon sizes from 16 to 1024 pixels were converted with `sips` and assembled with `iconutil`. `build_app.sh` embeds the ICNS in the app bundle and the source launch sets the same window/application icon.

## Document revision

Generated with the built-in image generator using the previous matte icon as a reference. Replace the large tags with a dominant upright periwinkle-blue file sheet, rounded corners and folded upper-right corner. Add a smaller lilac rectangular name label with a white curved arrow. Preserve matte ceramic material, warm ivory background, upper-left light, gentle shadows and a text-free design. Final asset: `renamer-icon.png`.

## Rounded macOS revision

The user requested a cut of the existing artwork, with no new generation. `examples/round_icon.rs` creates an antialiased rounded-square alpha mask, 50 px transparent inset and 208 px corner radius on a 1024 px canvas. The original opaque artwork is backed up as `renamer-icon-square-original.png`. All ICNS sizes now carry transparency; the bundle uses `RenamerRounded-v3.icns` and build 201 to refresh icon registration.
