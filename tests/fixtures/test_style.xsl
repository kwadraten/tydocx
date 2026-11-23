<?xml version="1.0" encoding="utf-8"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:b="http://schemas.openxmlformats.org/officeDocument/2006/bibliography">
  <xsl:output method="xml" encoding="utf-8"/>
  <xsl:template name="templ_prop_StyleName">
    <b:StyleName>Test Style</b:StyleName>
  </xsl:template>
  <xsl:template match="/">
    <b:Sources SelectedStyle="\TestStyle" StyleName="Test Style"/>
  </xsl:template>
</xsl:stylesheet>
