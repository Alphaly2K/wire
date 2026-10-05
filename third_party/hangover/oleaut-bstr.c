/*
 *	OLEAUT32
 *
 * Copyright 1999, 2000 Marcus Meissner
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

UINT WINAPI SysStringLen(BSTR str)
{
    return str ? bstr_from_str(str)->size/sizeof(WCHAR) : 0;
}

UINT WINAPI SysStringByteLen(BSTR str)
{
    return str ? bstr_from_str(str)->size : 0;
}

BSTR WINAPI SysAllocString(LPCOLESTR str)
{
    if (!str) return 0;

    /* Delegate this to the SysAllocStringLen32 method. */
    return SysAllocStringLen(str, lstrlenW(str));
}

BSTR WINAPI SysAllocStringLen(const OLECHAR *str, unsigned int len)
{
    bstr_t *bstr;
    DWORD size;

    /* Detect integer overflow. */
    if (len >= ((UINT_MAX-sizeof(WCHAR)-sizeof(DWORD))/sizeof(WCHAR)))
	return NULL;

    TRACE("%s\n", debugstr_wn(str, len));

    size = len*sizeof(WCHAR);
    bstr = alloc_bstr(size);
    if(!bstr)
        return NULL;

    if(str) {
        memcpy(bstr->u.str, str, size);
        bstr->u.str[len] = 0;
    }else {
        memset(bstr->u.str, 0, size+sizeof(WCHAR));
    }

    return bstr->u.str;
}

int WINAPI SysReAllocStringLen(BSTR* old, const OLECHAR* str, unsigned int len)
{
    /* Detect integer overflow. */
    if (len >= ((UINT_MAX-sizeof(WCHAR)-sizeof(DWORD))/sizeof(WCHAR)))
	return FALSE;

    if (*old!=NULL) {
      DWORD newbytelen = len*sizeof(WCHAR);
      bstr_t *old_bstr = bstr_from_str(*old);
      bstr_t *bstr = CoTaskMemRealloc(old_bstr, bstr_alloc_size(newbytelen));

      if (!bstr) return FALSE;

      *old = bstr->u.str;
      bstr->size = newbytelen;
      /* The old string data is still there when str is NULL */
      if (str && old_bstr->u.str != str) memmove(bstr->u.str, str, newbytelen);
      bstr->u.str[len] = 0;
    } else {
      *old = SysAllocStringLen(str, len);
    }

    return TRUE;
}

INT WINAPI SysReAllocString(LPBSTR old,LPCOLESTR str)
{
    /*
     * Sanity check
     */
    if (old==NULL)
      return 0;

    /*
     * Make sure we free the old string.
     */
    SysFreeString(*old);

    /*
     * Allocate the new string
     */
    *old = SysAllocString(str);

     return 1;
}
