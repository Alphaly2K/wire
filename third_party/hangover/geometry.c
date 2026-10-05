/*
 * Default window procedure
 *
 * Copyright 1993, 1996 Alexandre Julliard
 * Copyright 1995 Alex Korobka
 * Copyright 2022 Jacek Caban for CodeWeavers
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * This library is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with this library; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1301, USA
 */

BOOL adjust_window_rect( RECT *rect, DWORD style, BOOL menu, DWORD ex_style, UINT dpi )
{
    NONCLIENTMETRICSW ncm = {.cbSize = sizeof(ncm)};
    int adjust = 0;

    NtUserSystemParametersInfoForDpi( SPI_GETNONCLIENTMETRICS, 0, &ncm, 0, dpi );

    if ((ex_style & (WS_EX_STATICEDGE|WS_EX_DLGMODALFRAME)) == WS_EX_STATICEDGE)
        adjust = 1; /* for the outer frame always present */
    else if ((ex_style & WS_EX_DLGMODALFRAME) || (style & (WS_THICKFRAME|WS_DLGFRAME)))
        adjust = 2; /* outer */

    if (style & WS_THICKFRAME)
        adjust += ncm.iBorderWidth + ncm.iPaddedBorderWidth; /* The resize border */

    if ((style & (WS_BORDER|WS_DLGFRAME)) || (ex_style & WS_EX_DLGMODALFRAME))
        adjust++; /* The other border */

    InflateRect( rect, adjust, adjust );

    if ((style & WS_CAPTION) == WS_CAPTION)
    {
        if (ex_style & WS_EX_TOOLWINDOW)
            rect->top -= ncm.iSmCaptionHeight + 1;
        else
            rect->top -= ncm.iCaptionHeight + 1;
    }
    if (menu) rect->top -= ncm.iMenuHeight + 1;

    if (ex_style & WS_EX_CLIENTEDGE)
        InflateRect( rect, get_system_metrics(SM_CXEDGE), get_system_metrics(SM_CYEDGE) );
    return TRUE;
}
