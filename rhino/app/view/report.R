box::use(
  bslib[bind_task_button, card, card_body, card_header, input_task_button],
  mirai[mirai],
  shiny[
    conditionalPanel,
    downloadButton,
    downloadHandler,
    ExtendedTask,
    moduleServer,
    NS,
    observeEvent,
    outputOptions,
    reactive,
    renderText,
    req,
    tags,
    textOutput
  ]
)

#' @export
ui <- function(id) {
  ns <- NS(id)
  download <- downloadButton(ns("download"), "ダウンロード")
  download$attribs$target <- "_self"
  card(
    card_header(tags$h2(class = "h5 my-1", "集計レポート")),
    card_body(
      conditionalPanel(
        "!output.available",
        ns = ns,
        "サイコロを振るとレポートを作成できます。"
      ),
      conditionalPanel(
        "output.available",
        ns = ns,
        input_task_button(
          ns("render"),
          "レポートを作成",
          label_busy = "作成中…"
        )
      ),
      tags$div(`aria-live` = "polite", textOutput(ns("status"))),
      conditionalPanel(
        "output.ready",
        ns = ns,
        download
      )
    )
  )
}

#' @export
server <- function(id, counts, counter) {
  moduleServer(id, function(input, output, session) {
    root <- normalizePath(box::file("../.."))
    task <- ExtendedTask$new(function(value, distribution) {
      mirai(
        {
          options(box.path = root)
          box::use(app / report / render)
          render$dashboard(value, distribution, quarto)
        },
        root = root,
        value = value,
        distribution = distribution,
        quarto = Sys.getenv("SHINYTAURI_QUARTO", unset = Sys.which("quarto"))
      )
    }) |>
      bind_task_button("render")
    observeEvent(input$render, {
      task$invoke(as.integer(counter()), req(counts()))
    })
    output$available <- reactive(!is.null(counts()))
    output$ready <- reactive(identical(task$status(), "success"))
    outputOptions(output, "available", suspendWhenHidden = FALSE)
    outputOptions(output, "ready", suspendWhenHidden = FALSE)
    output$status <- renderText(switch(
      task$status(),
      running = "レポートを作成しています。",
      success = "作成時点の集計レポートをダウンロードできます。",
      error = "作成に失敗しました。もう一度レポートを作成してください。",
      ""
    ))
    output$download <- downloadHandler(
      filename = "dice-dashboard.html",
      contentType = "text/html; charset=utf-8",
      content = function(file) writeBin(task$result(), file)
    )
  })
}
