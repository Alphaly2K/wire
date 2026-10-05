/*
 * Window related functions
 *
 * Copyright 1993, 1994 Alexandre Julliard
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

/* Unmodified excerpts from dlls/user32/win.c, Wine 10.0. Not built. */
/*******************************************************************
 *           enum_windows
 */
static BOOL enum_windows( HDESK desktop, HWND hwnd, DWORD tid, BOOL children,
                          WNDENUMPROC proc, LPARAM param )
{
    HWND *list;
    ULONG i, size = 128;
    BOOL ret = !children;  /* EnumChildWindows returns FALSE on empty list, the others TRUE */
    NTSTATUS status;

    for (;;)
    {
        if (!(list = HeapAlloc( GetProcessHeap(), 0, size * sizeof(HWND) ))) return FALSE;
        status = NtUserBuildHwndList( desktop, hwnd, children, TRUE, tid, size, list, &size );
        if (!status) break;
        HeapFree( GetProcessHeap(), 0, list );
        if (status != STATUS_BUFFER_TOO_SMALL) return FALSE;
    }
    for (i = 0; i < size && list[i] != HWND_BOTTOM; i++)
    {
        if (!IsWindow( list[i] )) continue;
        if (!(ret = enum_callback_wrapper( proc, list[i], param ))) break;
    }
    HeapFree( GetProcessHeap(), 0, list );
    return ret;
}

/*******************************************************************
 *		EnumWindows (USER32.@)
 */
BOOL WINAPI EnumWindows( WNDENUMPROC lpEnumFunc, LPARAM lParam )
{
    return enum_windows( 0, 0, 0, FALSE, lpEnumFunc, lParam );
}


/**********************************************************************
 *		EnumThreadWindows (USER32.@)
 */
BOOL WINAPI EnumThreadWindows( DWORD id, WNDENUMPROC func, LPARAM lParam )
{
    return enum_windows( 0, 0, id, FALSE, func, lParam );
}
