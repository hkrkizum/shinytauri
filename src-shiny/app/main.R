box::use(
  bslib[bs_theme, layout_columns, page_fluid],
  shiny[moduleServer, NS, renderUI, tags, uiOutput],
  shiny.router[get_page, page404, route, route_link, router_server, router_ui]
)

box::use(app / types[typed])

box::use(app / view / counter, app / view / dice, app / view / report)

nav_link <- function(path, label, current) {
  active <- identical(path, current)
  tags$a(
    href = route_link(path),
    class = if (active) "nav-link active" else "nav-link",
    `aria-current` = if (active) "page",
    label
  )
}

#' Build the application shell and its two routes
#' @param id Shiny module ID supplied by Rhino.
#' @return The application page.
#' @export
ui <- function(id) {
  ns <- NS(typed(id, S7::class_character))
  pages <- router_ui(
    route(
      "/",
      layout_columns(
        col_widths = c(8, 4),
        class = "align-items-start",
        fill = FALSE,
        dice$ui(ns("dice")),
        counter$ui(ns("counter"))
      )
    ),
    route("report", report$ui(ns("report"))),
    page_404 = page404(message404 = "ページが見つかりません")
  )
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
      uiOutput(ns("navigation")),
      pages
    )
  ) |>
    typed(S7::new_S3_class("shiny.tag.list"))
}

#' Connect session-scoped modules to the router
#' @param id Shiny module ID supplied by Rhino.
#' @return Invisible NULL after registering the session outputs.
#' @export
server <- function(id) {
  moduleServer(
    typed(id, S7::class_character),
    function(input, output, session) {
      router_server()
      output$navigation <- renderUI({
        current <- get_page()
        tags$nav(
          class = "nav nav-pills gap-2 mb-3",
          `aria-label` = "ページ",
          nav_link("/", "サイコロ集計", current),
          nav_link("report", "レポート", current)
        )
      })
      distribution <- dice$server("dice")
      value <- counter$server("counter")
      report$server("report", distribution, value)
      invisible(NULL)
    }
  )
}
