.PHONY: serve build deploy thumbnails clean

# Documentation dev server
serve:
	cd doc && uv run zensical serve

# Build the documentation site
build:
	cd doc && uv run zensical build

# Deploy to surge.sh (only the built site)
deploy: build
	cd doc/site && npx surge . offworld-nexus-dhd.surge.sh

# Regenerate STL thumbnails from hardware/stl/*.stl
thumbnails:
	@mkdir -p doc/img/stl doc/docs/img/stl
	@for stl in hardware/stl/*.stl; do \
		name=$$(basename "$$stl" .stl); \
		echo "Rendering $$name..."; \
		openscad -o "doc/img/stl/$${name}.png" \
			--autocenter --viewall \
			--camera=0,0,0,55,0,25,0 \
			--imgsize=800,600 \
			--colorscheme=Tomorrow \
			<(echo "import(\"$$stl\");"); \
	done
	@cp doc/img/stl/*.png doc/docs/img/stl/
	@echo "Done."

# Convert HEIC images to JPG (strips EXIF)
images:
	@for heic in doc/img/*.heic; do \
		[ -f "$$heic" ] || continue; \
		jpg="$${heic%.heic}.jpg"; \
		echo "Converting $$heic -> $$jpg"; \
		convert "$$heic" -quality 85 "$$jpg"; \
		mogrify -strip "$$jpg"; \
		rm "$$heic"; \
	done
	@cp doc/img/*.jpg doc/docs/img/ 2>/dev/null || true
	@echo "Done."

# Clean build artifacts
clean:
	rm -rf doc/site
