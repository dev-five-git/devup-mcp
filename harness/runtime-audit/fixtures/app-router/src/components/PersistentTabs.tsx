'use client';
import Link from 'next/link';
import {usePathname} from 'next/navigation';
import {Flex,Text,Box} from '@devup-ui/react';
export function PersistentTabs(){
  const pathname=usePathname();
  return <Flex as="nav" id="consumer-tabs" aria-label="Consumer pages" gap="16px" p="16px" pos="relative">
    <Text as={Link} props={{href:'/', 'aria-current':pathname==='/'?'page':undefined}} color={pathname==='/'?'$brand':'$title'}>Summary</Text>
    <Text as={Link} props={{href:'/details', 'aria-current':pathname==='/details'?'page':undefined}} color={pathname==='/details'?'$brand':'$title'}>Details</Text>
    <Box as="span" id="consumer-tab-indicator" aria-hidden="true" pos="absolute" left="0" bottom="8px" h="2px" bg="$brand"/>
  </Flex>;
}
