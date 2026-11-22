library(shiny)

shinyApp(
  ui = fluidPage(
    title = "shinytauri カウンター",
    lang = "ja",
    tags$link(rel = "stylesheet", href = "style.css"),
    tags$header(
      class = "app-header",
      tags$div(class = "app-shell", tags$span(class = "app-name", "shinytauri"))
    ),
    tags$main(
      class = "app-shell counter-page",
      tags$h1("カウンター"),
      tags$section(
        class = "counter-panel",
        `aria-labelledby` = "count-label",
        tags$div(
          tags$h2(id = "count-label", "現在のカウント"),
          tags$div(role = "status", `aria-atomic` = "true", textOutput("total"))
        ),
        actionButton("count", "1 を足す", class = "counter-action")
      )
    )
  ),
  server = function(input, output, session) {
    output$total <- renderText(as.integer(input$count))
  }
)
