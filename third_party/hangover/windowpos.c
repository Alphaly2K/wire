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

static void handle_window_pos_changed( HWND hwnd, const WINDOWPOS *winpos )
{
    RECT rect;

    get_client_rect_rel( hwnd, COORDS_PARENT, &rect, get_thread_dpi() );
    if (!(winpos->flags & SWP_NOCLIENTMOVE))
        send_message( hwnd, WM_MOVE, 0, MAKELONG( rect.left, rect.top ));

    if (!(winpos->flags & SWP_NOCLIENTSIZE) || (winpos->flags & SWP_STATECHANGED))
    {
        if (is_iconic( hwnd ))
        {
            send_message( hwnd, WM_SIZE, SIZE_MINIMIZED, 0 );
        }
        else
        {
            WPARAM wp = is_zoomed( hwnd ) ? SIZE_MAXIMIZED : SIZE_RESTORED;
            send_message( hwnd, WM_SIZE, wp,
                          MAKELONG( rect.right-rect.left, rect.bottom-rect.top ));
        }
    }
}
