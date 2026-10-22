// Syntax themes built from the know-me tokens. Every token colour passes
// WCAG AA (4.5:1) on the code surface. In light mode the Ember and Cyan
// tokens fall below 4.5:1 as text, so light code is monochrome and uses
// weight and style instead of hue.
const dark = {
  plain: {color: '#E8EDF3', backgroundColor: '#161D29'},
  styles: [
    {types: ['comment', 'prolog', 'doctype', 'cdata'], style: {color: '#A7B0BC', fontStyle: 'italic'}},
    {types: ['keyword', 'builtin', 'important', 'atrule'], style: {color: '#FF6A3D'}},
    {types: ['string', 'char', 'attr-value', 'regex', 'url'], style: {color: '#00C2DC'}},
    {types: ['number', 'boolean', 'constant', 'symbol'], style: {color: '#22C55E'}},
    {types: ['function', 'class-name', 'macro'], style: {color: '#E8EDF3', fontWeight: '600'}},
    {types: ['punctuation', 'operator'], style: {color: '#A7B0BC'}},
    {types: ['deleted'], style: {color: '#EF4444'}},
    {types: ['inserted'], style: {color: '#22C55E'}},
  ],
};

const light = {
  plain: {color: '#0B0F14', backgroundColor: '#FAFBFC'},
  styles: [
    {types: ['comment', 'prolog', 'doctype', 'cdata'], style: {color: '#4B5563', fontStyle: 'italic'}},
    {types: ['keyword', 'builtin', 'important', 'atrule'], style: {color: '#0B0F14', fontWeight: '700'}},
    {types: ['string', 'char', 'attr-value', 'regex', 'url'], style: {color: '#4B5563'}},
    {types: ['number', 'boolean', 'constant', 'symbol'], style: {color: '#0B0F14', fontWeight: '600'}},
    {types: ['function', 'class-name', 'macro'], style: {color: '#0B0F14', fontWeight: '600'}},
    {types: ['punctuation', 'operator'], style: {color: '#4B5563'}},
    {types: ['deleted'], style: {color: '#0B0F14', textDecorationLine: 'line-through'}},
    {types: ['inserted'], style: {color: '#0B0F14', fontWeight: '600'}},
  ],
};

export const codeThemes = {dark, light};
