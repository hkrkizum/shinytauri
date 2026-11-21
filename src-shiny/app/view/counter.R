box::use(
  bslib[card, card_body, card_header],
  shiny[actionButton, moduleServer, NS, reactive, renderText, tags, textOutput]
)

box::use(.. / types[typed, Tag, Reactive])

#' Display the counter controls
#' @param id Shiny module ID.
#' @return A Bootstrap card.
#' @export
ui <- function(id) {
  ns <- NS(typed(id, S7::class_character))
  card(
    card_header(tags$h2(class = "h5 my-1", "カウンター")),
    card_body(
      tags$p("集計中も操作できます。"),
      tags$div(class = "h4 text-primary", textOutput(ns("count"))),
      actionButton(ns("increment"), "1 を足す", class = "align-self-start")
    )
  ) |>
    typed(Tag)
}

#' Track the session counter
#' @param id Shiny module ID.
#' @return A reactive expression containing the current integer count.
#' @export
server <- function(id) {
  moduleServer(
    typed(id, S7::class_character),
    function(input, output, session) {
      value <- reactive(as.integer(input$increment))
      output$count <- renderText(sprintf("カウント：%d", value()))
      value
    }
  ) |>
    typed(Reactive)
}
