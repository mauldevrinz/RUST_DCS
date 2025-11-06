#!/bin/bash
# Script untuk compile laporan LaTeX dengan bibliography

echo "=== Compiling LaTeX Report ==="
echo "Step 1: First LaTeX compilation..."
pdflatex -interaction=nonstopmode laporan.tex

echo ""
echo "Step 2: Processing bibliography with BibTeX..."
bibtex laporan

echo ""
echo "Step 3: Second LaTeX compilation..."
pdflatex -interaction=nonstopmode laporan.tex

echo ""
echo "Step 4: Final LaTeX compilation..."
pdflatex -interaction=nonstopmode laporan.tex

echo ""
echo "=== Compilation Complete! ==="
echo "Output: laporan.pdf"
