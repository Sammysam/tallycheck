# User guide

`Cenango-Accounting-User-Guide.pdf` is the illustrated user guide for the app: what it can do and how to use it, from the first start to month-end.

* `user-guide.html` is the source. Edit it, then rebuild the PDF.
* `images/` holds the screenshots. They come from the UI smoke test (`tests/ui/smoke.mjs`), which runs against a fresh install with the made-up company "Acme Trading (Pvt) Ltd". The 2FA key in `two-factor.jpg` is blurred.

Rebuild the PDF with Microsoft Edge:

```
"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --headless=new --no-pdf-header-footer ^
  --print-to-pdf="docs\user-guide\Cenango-Accounting-User-Guide.pdf" "file:///C:/dev/accounting-desktop/docs/user-guide/user-guide.html"
```

The page-number footer was added afterwards with PyMuPDF (`page.insert_text`). It is optional.
