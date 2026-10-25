// Folio: PDFs open in the app's own PDF.js reader, so foliate's PDF
// support (and its vendored copy of PDF.js) is left out.
export const makePDF = async () => {
    throw new Error('PDFs open in the PDF reader')
}
