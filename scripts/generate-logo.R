# Run from the repository root. The SVG artwork is licensed CC BY 4.0.
library(ggplot2)

navy <- "#182C3C"
cream <- "#FFF5DE"
teal <- "#14766B"
accent <- "#D37848"

radius <- 100
angles <- pi / 180 * seq(30, 330, by = 60)
hex <- data.frame(x = radius * cos(angles), y = radius * sin(angles))
rim <- 0.92 * hex

window <- data.frame(
  x = c(-58, 58, 65, 65, 58, -58, -65, -65),
  y = c(58, 58, 51, -7, -14, -14, -7, 51)
)
plot_line <- data.frame(
  x = c(-47, -27, -8, 12, 29, 48),
  y = c(4, 17, 12, 26, 20, 36)
)

logo <- ggplot() +
  geom_polygon(aes(x, y), data = hex, fill = navy) +
  geom_polygon(
    aes(x, y),
    data = rim,
    fill = NA,
    colour = "#719E9B",
    linewidth = 0.5
  ) +
  geom_polygon(aes(x, y), data = window, fill = cream) +
  annotate(
    "segment",
    x = -65,
    xend = 65,
    y = 43,
    yend = 43,
    colour = "#BBC7BB",
    linewidth = 0.5
  ) +
  annotate("point", x = c(-54, -45, -36), y = 50.5, colour = teal, size = 1.1) +
  annotate(
    "segment",
    x = -48,
    xend = 51,
    y = 1,
    yend = 1,
    colour = "#BBC7BB",
    linewidth = 0.45
  ) +
  geom_path(
    aes(x, y),
    data = plot_line,
    colour = teal,
    linewidth = 1.6,
    lineend = "round",
    linejoin = "round"
  ) +
  annotate("point", x = 48, y = 36, colour = accent, size = 2.9) +
  annotate(
    "text",
    x = 0,
    y = -37,
    label = "shinytauri",
    family = "Inter",
    fontface = "bold",
    size = 7.5,
    colour = cream
  ) +
  coord_fixed(xlim = c(-89, 89), ylim = c(-102, 102), expand = FALSE) +
  theme_void() +
  theme(plot.margin = margin(0, 0, 0, 0))

output <- "docs/images/shinytauri.svg"
ggsave(
  output,
  logo,
  device = grDevices::svg,
  width = 2.67,
  height = 3.06,
  bg = "transparent"
)

# Keep attribution discoverable when the SVG is copied out of the repository.
svg <- readLines(output, encoding = "UTF-8")
writeLines(
  append(
    svg,
    c(
      "<!-- SPDX-License-Identifier: CC-BY-4.0",
      "     Copyright (c) 2026 Hikaru Koizumi",
      "     https://hkrkizum.github.io/shinytauri/license.html -->",
      "<title>shinytauri</title>"
    ),
    after = which(startsWith(svg, "<svg "))
  ),
  output,
  useBytes = TRUE
)
