echo "Removing Plasma wallpapers ..."
# So that even when plasma-workspace-wallpapers were not installed, oma could
# still complete the removal procedure (showing a "package A is not installed"
# status) successfully.
oma refresh
oma purge plasma-workspace-wallpapers --yes
