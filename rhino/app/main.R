box::use(
  bslib[bs_theme, card, card_body, card_header, layout_columns, page_fluid],
  shiny[actionButton, moduleServer, NS, renderText, tags, textOutput]
)

box::use(app / view / dice)

#' @export
ui <- function(id) {
  ns <- NS(id)
  page_fluid(
    title = "shinytauri",
    lang = "ja",
    theme = bs_theme(
      version = 5,
      bg = "#f3f6f5",
      fg = "#203833",
      primary = "#12695b",
      "card-bg" = "#ffffff"
    ),
    tags$main(
      class = "container-lg py-4",
      tags$h1(class = "h3 fw-bold mb-4", "shinytauri"),
      layout_columns(
        col_widths = c(8, 4),
        class = "align-items-start",
        fill = FALSE,
        dice$ui(ns("dice")),
        card(
          card_header(tags$h2(class = "h5 my-1", "カウンター")),
          card_body(
            tags$p("集計中も操作できます。"),
            tags$div(class = "h4 text-primary", textOutput(ns("count"))),
            actionButton(
              ns("increment"),
              "1 を足す",
              class = "align-self-start"
            )
          )
        )
      )
    )
  )
}

#' @export
server <- function(id) {
  moduleServer(id, function(input, output, session) {
    dice$server("dice")
    output$count <- renderText(sprintf("カウント：%d", input$increment))
  })
}
